use crossterm::event::{KeyCode, KeyEvent};

use allthecodes_config::settings::{
    ProviderProfileReplacement, ProviderProfileSettings, ProviderProfileStore,
    ProviderSecretUpdates, SecretUpdate,
};

use crate::ui::better_view_panel::{plain_row, selected_row, BetterViewPanel};
use crate::ui::command_surface::{cycle_index, CommandSurfaceOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileRow {
    id: String,
    active: bool,
    provider: String,
    runtime_support: String,
    model: Option<String>,
    base_url: Option<String>,
    credential_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PresetRow {
    id: String,
    label: String,
    base_url: Option<String>,
    default_model: Option<String>,
    supported: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ProfileDraft {
    id: String,
    provider: String,
    model: String,
    base_url: String,
    api_key: String,
    clear_api_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProvidersMode {
    Profiles,
    Presets,
    Detail(usize),
    PresetDetail(usize),
    Edit {
        target: Option<String>,
        draft: ProfileDraft,
        step: usize,
    },
    DeleteConfirm(usize),
    Result(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidersSurface {
    profiles: Vec<ProfileRow>,
    presets: Vec<PresetRow>,
    selected: usize,
    mode: ProvidersMode,
    load_error: Option<String>,
}

impl ProvidersSurface {
    pub(crate) fn new() -> Self {
        let mut surface = Self {
            profiles: Vec::new(),
            presets: preset_rows(),
            selected: 0,
            mode: ProvidersMode::Profiles,
            load_error: None,
        };
        surface.reload();
        surface
    }

    fn reload(&mut self) {
        match ProviderProfileStore::global().list() {
            Ok(profiles) => {
                self.profiles = profiles
                    .into_iter()
                    .map(|profile| ProfileRow {
                        id: profile.id,
                        active: profile.active,
                        provider: profile.api_provider.unwrap_or_else(|| "unset".to_string()),
                        runtime_support: profile.runtime_support,
                        model: profile.model,
                        base_url: profile.base_url,
                        credential_configured: profile.api_key_configured
                            || !profile.env_keys.is_empty()
                            || profile.auth_source_configured,
                    })
                    .collect();
                self.selected = self.selected.min(self.profiles.len().saturating_sub(1));
                self.load_error = None;
            }
            Err(error) => self.load_error = Some(error.to_string()),
        }
    }

    pub(crate) fn render(&self) -> String {
        match &self.mode {
            ProvidersMode::Profiles => self.render_profiles(),
            ProvidersMode::Presets => self.render_presets(),
            ProvidersMode::Detail(index) => self.render_detail(*index),
            ProvidersMode::PresetDetail(index) => self.render_preset_detail(*index),
            ProvidersMode::Edit {
                target,
                draft,
                step,
            } => self.render_edit(target.as_deref(), draft, *step),
            ProvidersMode::DeleteConfirm(index) => self.render_delete(*index),
            ProvidersMode::Result(message) => BetterViewPanel::new("Providers / Result")
                .summary("profile storage updated")
                .sections_title("Status")
                .sections(vec!["Result".to_string()], 0)
                .detail_title("Message")
                .detail_lines(message.lines().map(str::to_string).collect())
                .footer("Enter/Backspace profiles | Esc close")
                .render(),
        }
    }

    fn render_profiles(&self) -> String {
        let mut lines = if self.profiles.is_empty() {
            vec!["No configured provider profiles".to_string()]
        } else {
            self.profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| {
                    selected_row(
                        format!("{}{}", if profile.active { "* " } else { "  " }, profile.id),
                        format!(
                            "{}  runtime={}  credential={}",
                            profile.provider,
                            profile.runtime_support,
                            if profile.credential_configured {
                                "configured"
                            } else {
                                "missing"
                            }
                        ),
                        index == self.selected,
                    )
                })
                .collect()
        };
        if let Some(error) = &self.load_error {
            lines.push(format!("error: {error}"));
        }
        BetterViewPanel::new("Providers / Configured")
            .summary(format!("profiles={} presets={}", self.profiles.len(), self.presets.len()))
            .sections_title("Views")
            .sections(vec!["Configured".to_string(), "Presets (read-only)".to_string()], 0)
            .detail_title("Provider profiles")
            .detail_lines(lines)
            .footer("Enter detail | u activate | n create | e replace | d delete | p presets | r reload | Esc close")
            .render()
    }

    fn render_presets(&self) -> String {
        let lines = self
            .presets
            .iter()
            .enumerate()
            .map(|(index, preset)| {
                selected_row(
                    &preset.label,
                    format!(
                        "{}  {}",
                        preset.id,
                        if preset.supported {
                            "supported"
                        } else {
                            "unsupported"
                        }
                    ),
                    index == self.selected,
                )
            })
            .collect();
        BetterViewPanel::new("Providers / Presets")
            .summary("built-in presets are read-only")
            .sections_title("Views")
            .sections(
                vec!["Configured".to_string(), "Presets (read-only)".to_string()],
                1,
            )
            .detail_title("Built-in provider presets")
            .detail_lines(lines)
            .footer("Enter detail | n create from preset | p configured | Esc close")
            .render()
    }

    fn render_detail(&self, index: usize) -> String {
        let Some(profile) = self.profiles.get(index) else {
            return "Provider profile no longer exists".to_string();
        };
        BetterViewPanel::new(format!("Providers / {}", profile.id))
            .summary(if profile.active {
                "active profile"
            } else {
                "configured profile"
            })
            .sections_title("Profile")
            .sections(vec![profile.id.clone()], 0)
            .detail_title("Redacted details")
            .detail_lines(vec![
                plain_row("provider:", &profile.provider),
                plain_row("runtime:", &profile.runtime_support),
                plain_row("model:", profile.model.as_deref().unwrap_or("unset")),
                plain_row(
                    "base URL:",
                    profile.base_url.as_deref().unwrap_or("default"),
                ),
                plain_row(
                    "credential:",
                    if profile.credential_configured {
                        "configured (redacted)"
                    } else {
                        "missing"
                    },
                ),
            ])
            .footer("u activate | e replace | d delete | Backspace list | Esc close")
            .render()
    }

    fn render_preset_detail(&self, index: usize) -> String {
        let Some(preset) = self.presets.get(index) else {
            return "Provider preset no longer exists".to_string();
        };
        BetterViewPanel::new(format!("Providers / Preset / {}", preset.label))
            .summary("read-only built-in preset")
            .sections_title("Preset")
            .sections(vec![preset.id.clone()], 0)
            .detail_title("Details")
            .detail_lines(vec![
                plain_row("provider:", &preset.id),
                plain_row(
                    "runtime:",
                    if preset.supported {
                        "supported"
                    } else {
                        "unsupported"
                    },
                ),
                plain_row(
                    "model:",
                    preset
                        .default_model
                        .as_deref()
                        .unwrap_or("provider default"),
                ),
                plain_row(
                    "base URL:",
                    preset.base_url.as_deref().unwrap_or("provider managed"),
                ),
            ])
            .footer("n create profile from preset | Backspace presets | Esc close")
            .render()
    }

    fn render_edit(&self, target: Option<&str>, draft: &ProfileDraft, step: usize) -> String {
        let labels = [
            "Profile ID",
            "Provider",
            "Model",
            "Base URL",
            "API key",
            "Confirm",
        ];
        let value = match step {
            0 => draft.id.clone(),
            1 => draft.provider.clone(),
            2 => draft.model.clone(),
            3 => draft.base_url.clone(),
            4 if draft.clear_api_key => "clear existing".to_string(),
            4 => "*".repeat(draft.api_key.chars().count()),
            _ => format!(
                "id: {}\nprovider: {}\nmodel: {}\nbase URL: {}\nAPI key: {}",
                draft.id,
                draft.provider,
                value_or_default(&draft.model),
                value_or_default(&draft.base_url),
                if draft.clear_api_key {
                    "clear existing"
                } else if draft.api_key.is_empty() {
                    if target.is_some() {
                        "keep existing"
                    } else {
                        "not set"
                    }
                } else {
                    "set new value"
                }
            ),
        };
        BetterViewPanel::new(if target.is_some() {
            "Providers / Replace"
        } else {
            "Providers / Create"
        })
        .summary(format!("step={}/6 secrets=redacted", step + 1))
        .sections_title("Fields")
        .sections(
            labels.iter().map(|value| (*value).to_string()).collect(),
            step,
        )
        .detail_title(labels[step])
        .detail_lines(value.lines().map(str::to_string).collect())
        .footer(if target.is_some() && step == 4 {
            "Type new key | Delete toggle clear existing | Enter next | Backspace edit/previous | Esc close"
        } else {
            "Type value | Enter next/save | Backspace edit/previous | Esc close"
        })
        .render()
    }

    fn render_delete(&self, index: usize) -> String {
        let Some(profile) = self.profiles.get(index) else {
            return "Provider profile no longer exists".to_string();
        };
        BetterViewPanel::new("Providers / Delete")
            .summary(if profile.active {
                "blocked: profile is active"
            } else {
                "destructive operation"
            })
            .sections_title("Target")
            .sections(vec![profile.id.clone()], 0)
            .detail_title("Confirmation")
            .detail_lines(vec![format!("Delete provider profile `{}`?", profile.id)])
            .footer(if profile.active {
                "Activate another profile first | Backspace cancel | Esc close"
            } else {
                "Enter delete | Backspace cancel | Esc close"
            })
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match self.mode.clone() {
            ProvidersMode::Profiles => self.handle_profiles(key),
            ProvidersMode::Presets => self.handle_presets(key),
            ProvidersMode::Detail(index) => self.handle_detail(index, key),
            ProvidersMode::PresetDetail(index) => self.handle_preset_detail(index, key),
            ProvidersMode::Edit {
                target,
                draft,
                step,
            } => self.handle_edit(target, draft, step, key),
            ProvidersMode::DeleteConfirm(index) => self.handle_delete(index, key),
            ProvidersMode::Result(_) => match key.code {
                KeyCode::Enter | KeyCode::Backspace => {
                    self.mode = ProvidersMode::Profiles;
                    CommandSurfaceOutcome::None
                }
                _ => CommandSurfaceOutcome::None,
            },
        }
    }

    fn handle_profiles(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = cycle_index(self.selected, self.profiles.len(), -1);
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected = cycle_index(self.selected, self.profiles.len(), 1);
            }
            KeyCode::Enter => self.mode = ProvidersMode::Detail(self.selected),
            KeyCode::Char('u') => return self.activate_selected(),
            KeyCode::Char('n') => self.start_create(None),
            KeyCode::Char('e') => self.start_replace(),
            KeyCode::Char('d') => self.mode = ProvidersMode::DeleteConfirm(self.selected),
            KeyCode::Char('p') => {
                self.selected = 0;
                self.mode = ProvidersMode::Presets;
            }
            KeyCode::Char('r') => self.reload(),
            _ => {}
        }
        CommandSurfaceOutcome::None
    }

    fn handle_presets(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = cycle_index(self.selected, self.presets.len(), -1)
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.selected = cycle_index(self.selected, self.presets.len(), 1)
            }
            KeyCode::Enter => self.mode = ProvidersMode::PresetDetail(self.selected),
            KeyCode::Char('n') => {
                self.start_create(self.presets.get(self.selected).map(|p| p.id.clone()))
            }
            KeyCode::Char('p') | KeyCode::Backspace => {
                self.selected = 0;
                self.mode = ProvidersMode::Profiles;
            }
            _ => {}
        }
        CommandSurfaceOutcome::None
    }

    fn handle_detail(&mut self, index: usize, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Char('u') => self.activate_index(index),
            KeyCode::Char('e') => {
                self.selected = index;
                self.start_replace();
                CommandSurfaceOutcome::None
            }
            KeyCode::Char('d') => {
                self.mode = ProvidersMode::DeleteConfirm(index);
                CommandSurfaceOutcome::None
            }
            KeyCode::Backspace => {
                self.mode = ProvidersMode::Profiles;
                CommandSurfaceOutcome::None
            }
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn handle_preset_detail(&mut self, index: usize, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Char('n') => {
                self.start_create(self.presets.get(index).map(|p| p.id.clone()));
            }
            KeyCode::Backspace => self.mode = ProvidersMode::Presets,
            _ => {}
        }
        CommandSurfaceOutcome::None
    }

    fn handle_edit(
        &mut self,
        target: Option<String>,
        mut draft: ProfileDraft,
        mut step: usize,
        key: KeyEvent,
    ) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Enter if step == 5 => {
                return self.save_draft(target.as_deref(), draft);
            }
            KeyCode::Enter => step += 1,
            KeyCode::Delete if step == 4 && target.is_some() => {
                draft.clear_api_key = !draft.clear_api_key;
                draft.api_key.clear();
            }
            KeyCode::Backspace => {
                let value = draft_field_mut(&mut draft, step);
                if value.is_empty() && step > 0 {
                    step -= 1;
                } else {
                    value.pop();
                }
            }
            KeyCode::Char(ch) if step < 5 => {
                if step == 4 {
                    draft.clear_api_key = false;
                }
                draft_field_mut(&mut draft, step).push(ch);
            }
            _ => {}
        }
        self.mode = ProvidersMode::Edit {
            target,
            draft,
            step,
        };
        CommandSurfaceOutcome::None
    }

    fn handle_delete(&mut self, index: usize, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Enter => {
                let Some(profile) = self.profiles.get(index).cloned() else {
                    self.mode = ProvidersMode::Result("Profile no longer exists".to_string());
                    return CommandSurfaceOutcome::None;
                };
                let message = match ProviderProfileStore::global().delete(&profile.id) {
                    Ok(()) => format!("Deleted provider profile `{}`.", profile.id),
                    Err(error) => format!("Delete failed: {error}"),
                };
                self.reload();
                self.mode = ProvidersMode::Result(message);
            }
            KeyCode::Backspace => self.mode = ProvidersMode::Profiles,
            _ => {}
        }
        CommandSurfaceOutcome::None
    }

    fn activate_selected(&self) -> CommandSurfaceOutcome {
        self.activate_index(self.selected)
    }

    fn activate_index(&self, index: usize) -> CommandSurfaceOutcome {
        self.profiles
            .get(index)
            .map_or(CommandSurfaceOutcome::None, |profile| {
                CommandSurfaceOutcome::Submit(format!(
                    "/providers activate {}",
                    quote_id(&profile.id)
                ))
            })
    }

    fn start_create(&mut self, provider: Option<String>) {
        self.mode = ProvidersMode::Edit {
            target: None,
            draft: ProfileDraft {
                provider: provider.unwrap_or_default(),
                ..ProfileDraft::default()
            },
            step: 0,
        };
    }

    fn start_replace(&mut self) {
        let Some(profile) = self.profiles.get(self.selected).cloned() else {
            return;
        };
        self.mode = ProvidersMode::Edit {
            target: Some(profile.id.clone()),
            draft: ProfileDraft {
                id: profile.id,
                provider: profile.provider,
                model: profile.model.unwrap_or_default(),
                base_url: profile.base_url.unwrap_or_default(),
                api_key: String::new(),
                clear_api_key: false,
            },
            step: 0,
        };
    }

    fn save_draft(&mut self, target: Option<&str>, draft: ProfileDraft) -> CommandSurfaceOutcome {
        let id = draft.id.trim();
        let was_active = target.is_some_and(|target| {
            self.profiles
                .iter()
                .any(|profile| profile.id == target && profile.active)
        });
        let profile = ProviderProfileSettings {
            backend: Some(
                if draft.provider == "openai-codex" {
                    "codex"
                } else {
                    "native"
                }
                .to_string(),
            ),
            api_provider: non_empty(&draft.provider),
            model: non_empty(&draft.model),
            base_url: non_empty(&draft.base_url),
            api_key: if target.is_none() {
                non_empty(&draft.api_key)
            } else {
                None
            },
            ..ProviderProfileSettings::default()
        };
        let store = ProviderProfileStore::global();
        let result = if let Some(target) = target {
            store.replace_and_rename(
                target,
                id,
                ProviderProfileReplacement {
                    profile,
                    secrets: ProviderSecretUpdates {
                        api_key: draft_api_key_update(&draft),
                        ..ProviderSecretUpdates::default()
                    },
                },
            )
        } else {
            store.create(id, profile)
        };
        let saved = result.is_ok();
        let message = match result {
            Ok(()) => format!("Saved provider profile `{id}`."),
            Err(error) => format!("Save failed: {error}"),
        };
        self.reload();
        self.mode = ProvidersMode::Result(message);
        if saved && was_active {
            CommandSurfaceOutcome::Submit(format!("/providers activate {}", quote_id(id)))
        } else {
            CommandSurfaceOutcome::None
        }
    }
}

fn preset_rows() -> Vec<PresetRow> {
    let mut presets = allthecodes_api::api::providers::PROVIDERS
        .iter()
        .map(|provider| PresetRow {
            id: provider.name.to_string(),
            label: provider.label.to_string(),
            base_url: Some(provider.base_url.to_string()),
            default_model: Some(provider.default_model.to_string()),
            supported: true,
        })
        .collect::<Vec<_>>();
    presets.extend([
        PresetRow {
            id: "bedrock".to_string(),
            label: "Amazon Bedrock".to_string(),
            base_url: None,
            default_model: None,
            supported: true,
        },
        PresetRow {
            id: "vertex".to_string(),
            label: "Google Vertex AI".to_string(),
            base_url: None,
            default_model: None,
            supported: true,
        },
        PresetRow {
            id: "azure-foundry".to_string(),
            label: "Microsoft Foundry".to_string(),
            base_url: None,
            default_model: None,
            supported: false,
        },
    ]);
    presets
}

fn draft_field_mut(draft: &mut ProfileDraft, step: usize) -> &mut String {
    match step {
        0 => &mut draft.id,
        1 => &mut draft.provider,
        2 => &mut draft.model,
        3 => &mut draft.base_url,
        _ => &mut draft.api_key,
    }
}

fn non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn draft_api_key_update(draft: &ProfileDraft) -> SecretUpdate<String> {
    if draft.clear_api_key {
        SecretUpdate::Clear
    } else if draft.api_key.is_empty() {
        SecretUpdate::Keep
    } else {
        SecretUpdate::Set(draft.api_key.clone())
    }
}

fn value_or_default(value: &str) -> &str {
    if value.is_empty() {
        "default"
    } else {
        value
    }
}

fn quote_id(id: &str) -> String {
    format!("\"{}\"", id.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_is_masked_in_wizard_rendering() {
        let mut surface = ProvidersSurface::new();
        surface.mode = ProvidersMode::Edit {
            target: None,
            draft: ProfileDraft {
                api_key: "super-secret".to_string(),
                ..ProfileDraft::default()
            },
            step: 4,
        };
        let rendered = surface.render();
        assert!(!rendered.contains("super-secret"));
        assert!(rendered.contains("************"));
    }

    #[test]
    fn replacement_secret_action_distinguishes_keep_set_and_clear() {
        let mut draft = ProfileDraft::default();
        assert_eq!(draft_api_key_update(&draft), SecretUpdate::Keep);

        draft.api_key = "replacement".to_string();
        assert_eq!(
            draft_api_key_update(&draft),
            SecretUpdate::Set("replacement".to_string())
        );

        draft.api_key.clear();
        draft.clear_api_key = true;
        assert_eq!(draft_api_key_update(&draft), SecretUpdate::Clear);
    }
}
