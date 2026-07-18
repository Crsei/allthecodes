use crate::ui::notifications::in_app::{InAppNotification, NotificationPriority, NotificationTone};
use crate::ui::status_line::{
    build_payload_from_snapshot, payload, StatusLinePayload, StatusLineRunner, StatusLineSnapshot,
};
use allthecodes_config::settings::StatusLineSettings;

use super::App;
use super::GoalStatusSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextWindowSource {
    Exact,
    Estimated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextWindowPhase {
    Preparing,
    Streaming,
    Completed,
    Compacted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContextWindowSnapshot {
    pub model_id: String,
    pub used_tokens: u64,
    pub effective_capacity: Option<u64>,
    pub source: ContextWindowSource,
    pub phase: ContextWindowPhase,
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CumulativeContextUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_creation_tokens: u64,
    api_calls: u64,
}
/// Subset of engine usage-tracking relevant to the status-line payload.
/// Populated by [`App::update_session_usage`].
#[derive(Debug, Clone, Default)]
pub(super) struct SessionUsageSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub api_calls: u64,
    pub unknown_pricing_count: Option<u64>,
    pub backfilled_count: Option<u64>,
}

impl App {
    /// Update the accumulated session usage (tokens + api calls) used by
    /// the scriptable status-line payload.
    #[allow(clippy::too_many_arguments)]
    pub fn update_session_usage(
        &mut self,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        cache_creation_tokens: u64,
        api_calls: u64,
        unknown_pricing_count: Option<u64>,
        backfilled_count: Option<u64>,
    ) {
        self.session_usage = SessionUsageSnapshot {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            api_calls,
            unknown_pricing_count,
            backfilled_count,
        };
        // No dirty flip; `update_session_cost` already ran and marked it.
    }

    /// Set the effective denominator for the currently displayed model. The
    /// source is the same settings capability map used by the engine's model
    /// configuration surfaces; unknown models deliberately remain unknown.
    pub fn set_context_capacity_from_settings(
        &mut self,
        settings: &allthecodes_config::runtime_settings::SettingsJson,
    ) {
        let model = self.session_ui.model_name.clone();
        let capacity = resolve_context_capacity(&model, settings);
        let model_changed = self
            .context_capacity_model
            .as_deref()
            .is_some_and(|previous| previous != model);
        if model_changed || self.context_capacity != capacity {
            self.context_window_snapshot = None;
            self.dirty = true;
        }
        self.context_capacity_model = (!model.is_empty()).then_some(model);
        self.context_capacity = capacity;
    }

    /// Convert cumulative engine usage into a request-level delta so the
    /// status row never displays a session-wide accumulation.
    pub(crate) fn update_context_window_from_usage(
        &mut self,
        usage: &allthecodes_types::sdk::UsageTracking,
    ) {
        let current = CumulativeContextUsage {
            input_tokens: usage.total_input_tokens,
            output_tokens: usage.total_output_tokens,
            cache_read_tokens: usage.total_cache_read_tokens,
            cache_creation_tokens: usage.total_cache_creation_tokens,
            api_calls: usage.api_call_count,
        };
        let previous = self.context_usage_cursor;
        let used_tokens = current
            .input_tokens
            .saturating_sub(previous.input_tokens)
            .saturating_add(current.output_tokens.saturating_sub(previous.output_tokens))
            .saturating_add(
                current
                    .cache_read_tokens
                    .saturating_sub(previous.cache_read_tokens),
            )
            .saturating_add(
                current
                    .cache_creation_tokens
                    .saturating_sub(previous.cache_creation_tokens),
            );
        self.context_usage_cursor = current;
        self.context_window_snapshot = Some(ContextWindowSnapshot {
            model_id: self.session_ui.model_name.clone(),
            used_tokens,
            effective_capacity: self.context_capacity,
            source: if current.api_calls.saturating_sub(previous.api_calls) <= 1 {
                ContextWindowSource::Exact
            } else {
                ContextWindowSource::Estimated
            },
            phase: ContextWindowPhase::Completed,
        });
        self.dirty = true;
    }

    pub(super) fn mark_context_streaming(&mut self) {
        if let Some(snapshot) = &mut self.context_window_snapshot {
            snapshot.phase = ContextWindowPhase::Streaming;
        }
        self.dirty = true;
    }

    pub(crate) fn mark_context_compacted(&mut self, post_compact_tokens: Option<u64>) {
        if self.context_window_snapshot.is_some() || post_compact_tokens.is_some() {
            self.context_window_snapshot = Some(ContextWindowSnapshot {
                model_id: self.session_ui.model_name.clone(),
                // A compact boundary carries the estimate for the actual
                // post-compact message set. Without metadata, display
                // "calculating" rather than retaining a stale pre-compact
                // request value.
                used_tokens: post_compact_tokens.unwrap_or(0),
                effective_capacity: self.context_capacity,
                source: ContextWindowSource::Estimated,
                phase: ContextWindowPhase::Compacted,
            });
        }
        self.dirty = true;
    }

    pub(crate) fn prepare_context_window(&mut self) {
        if self.context_window_snapshot.is_none() && self.context_capacity.is_some() {
            self.context_window_snapshot = Some(ContextWindowSnapshot {
                model_id: self.session_ui.model_name.clone(),
                used_tokens: 0,
                effective_capacity: self.context_capacity,
                source: ContextWindowSource::Estimated,
                phase: ContextWindowPhase::Preparing,
            });
        }
        self.dirty = true;
    }

    pub fn update_goal_status(&mut self, event: &str, goal: &serde_json::Value) {
        let objective = goal
            .get("objective")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("session goal")
            .trim()
            .to_string();
        let status = goal
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(event)
            .to_string();
        let tokens_used = goal
            .get("tokens_used")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let time_used_seconds = goal
            .get("time_used_seconds")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default();
        let next = Some(GoalStatusSnapshot {
            event: event.to_string(),
            objective,
            status,
            tokens_used,
            time_used_seconds,
        });
        if self.active_goal != next {
            self.active_goal = next;
            self.dirty = true;
        }
    }

    /// Replace the resolved status-line settings (e.g. after `/statusline`
    /// edits the config).
    pub fn set_status_line_settings(&mut self, settings: StatusLineSettings) {
        self.status_line_settings = settings;
        // Drop any stale output so we fall back immediately if the user
        // disabled or cleared the command.
        if !self.status_line_settings.is_command_mode() {
            self.status_line_runner.reset();
        }
        self.dirty = true;
    }

    /// Mirror runtime-only settings into the built-in footer. The custom
    /// status-line payload has richer JSON; these labels keep the fallback
    /// footer useful when no command status line is configured.
    pub fn sync_status_context_from_state(
        &mut self,
        state: &allthecodes_engine::types::app_state::AppState,
    ) {
        let permission_mode = state.tool_permission_context.mode.as_str().to_string();
        let sandbox = sandbox_label(&state.settings.sandbox);
        // Display label routes through the transport-aware central resolver so
        // the status line never shows a Codex-side `output_config.effort` that
        // the resolver would not actually send on the wire (plan §3 phase D4).
        let effort = crate::startup_model::resolve_display_effort_label(state)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let remote_indicator = remote_indicator_label(state);

        if self.permission_mode_label != permission_mode
            || self.sandbox_label != sandbox
            || self.effort_label != effort
            || self.remote_indicator_label != remote_indicator
        {
            self.permission_mode_label = permission_mode;
            self.sandbox_label = sandbox;
            self.effort_label = effort;
            self.remote_indicator_label = remote_indicator;
            self.dirty = true;
        }

        if self.verbose != state.verbose {
            self.verbose = state.verbose;
            self.conversation.invalidate_vscroll_all();
            self.dirty = true;
        }

        if self.brief_only != state.is_brief_only {
            self.brief_only = state.is_brief_only;
            self.conversation.invalidate_vscroll_all();
            self.dirty = true;
        }

        if state.verbose {
            self.add_notification(
                InAppNotification::new(
                    "verbose-mode-indicator",
                    NotificationPriority::Low,
                    "Verbose mode enabled",
                )
                .with_tone(NotificationTone::Dim)
                .with_fold(true),
            );
        } else {
            self.remove_notification("verbose-mode-indicator");
        }
    }

    /// Shared handle to the status-line runner. `/statusline` calls this
    /// to inspect / reset the runner without owning the App.
    pub fn status_line_runner(&self) -> StatusLineRunner {
        self.status_line_runner.clone()
    }

    /// Adopt a pre-existing runner (e.g. the one stored on [`AppState`])
    /// so every UI surface and the `/statusline` command observe the same
    /// subprocess state.
    pub fn set_status_line_runner(&mut self, runner: StatusLineRunner) {
        self.status_line_runner = runner;
    }

    /// Build the current status-line payload from app state.
    pub(super) fn build_status_payload(&self) -> StatusLinePayload {
        let mut payload = build_payload_from_snapshot(StatusLineSnapshot {
            session_id: (!self.session_ui.session_id.is_empty())
                .then(|| self.session_ui.session_id.clone()),
            model_id: &self.session_ui.model_name,
            backend: (!self.session_ui.backend_name.is_empty())
                .then_some(self.session_ui.backend_name.as_str()),
            cwd: std::path::Path::new(&self.session_ui.cwd),
            input_tokens: self.session_usage.input_tokens,
            output_tokens: self.session_usage.output_tokens,
            cache_read_tokens: self.session_usage.cache_read_tokens,
            cache_creation_tokens: self.session_usage.cache_creation_tokens,
            total_cost_usd: self.session_cost_usd,
            api_calls: self.session_usage.api_calls,
            unknown_pricing_count: self.session_usage.unknown_pricing_count,
            backfilled_count: self.session_usage.backfilled_count,
            session_duration_secs: None,
            resolved_output_style_name: crate::ui::status_line_resolver::resolve_output_style_name(
                self.session_ui.output_style.as_deref(),
                std::path::Path::new(&self.session_ui.cwd),
            ),
            editor_mode: self.vim.enabled.then_some("vim"),
            worktree: crate::ui::status_line_resolver::current_worktree_status_for_session(
                (!self.session_ui.session_id.is_empty())
                    .then_some(self.session_ui.session_id.as_str()),
            ),
            streaming: self.is_streaming,
            message_count: self.conversation.messages().len(),
        });
        if self.vim.enabled {
            payload.vim = Some(payload::VimStatus {
                mode: self.vim.mode.indicator().to_string(),
            });
        }
        payload
    }

    /// Kick the runner. Throttling / cancellation lives inside the runner.
    pub(super) fn trigger_status_refresh(&self) {
        if !self.status_line_settings.is_command_mode() {
            return;
        }
        let payload = self.build_status_payload();
        let _ = self
            .status_line_runner
            .refresh(&self.status_line_settings, &payload);
    }
}

const _: fn(&App) -> StatusLineRunner = App::status_line_runner;

fn remote_indicator_label(
    state: &allthecodes_engine::types::app_state::AppState,
) -> Option<String> {
    if !state.kairos_active {
        return Some("off".to_string());
    }

    Some("attention".to_string())
}

fn sandbox_label(settings: &allthecodes_config::settings::SandboxSettings) -> String {
    if !settings.enabled.unwrap_or(false) {
        return "off".to_string();
    }

    let mode = settings.mode.as_deref().unwrap_or("workspace");
    if settings.network.disabled.unwrap_or(false) {
        format!("{mode},no-net")
    } else {
        mode.to_string()
    }
}

fn resolve_context_capacity(
    model: &str,
    settings: &allthecodes_config::runtime_settings::SettingsJson,
) -> Option<u64> {
    if model.is_empty() {
        return None;
    }
    let capability = settings.model_capabilities.get(model).or_else(|| {
        settings
            .active_auth_profile
            .as_deref()
            .and_then(|profile| settings.auth_profiles.get(profile))
            .and_then(|profile| profile.model_capabilities.as_ref())
            .and_then(|capabilities| capabilities.get(model))
    });
    let base = capability
        .and_then(|value| value.max_context_window.or(value.context_window))
        .or(settings.context_window)
        .filter(|value| *value > 0)?;
    let percent = capability
        .and_then(|value| value.effective_context_window_percent)
        .unwrap_or(100)
        .clamp(1, 100) as u64;
    Some(base.saturating_mul(percent) / 100)
}

pub(super) fn format_context_usage(snapshot: &ContextWindowSnapshot) -> String {
    if snapshot.used_tokens == 0
        && matches!(
            snapshot.phase,
            ContextWindowPhase::Preparing
                | ContextWindowPhase::Streaming
                | ContextWindowPhase::Compacted
        )
    {
        return "calculating".to_string();
    }
    let used = format_compact_tokens(snapshot.used_tokens);
    let Some(capacity) = snapshot.effective_capacity else {
        return format!("{used}/unknown");
    };
    let percent = if capacity == 0 {
        None
    } else {
        Some(snapshot.used_tokens.saturating_mul(100) / capacity)
    };
    match percent {
        Some(percent) if snapshot.used_tokens > capacity => {
            format!("{used}/{} (>{percent}%)", format_compact_tokens(capacity))
        }
        Some(percent) => format!("{used}/{} ({percent}%)", format_compact_tokens(capacity)),
        None => format!("{used}/unknown"),
    }
}

fn format_compact_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}m", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::sdk::UsageTracking;

    fn usage(
        input: u64,
        output: u64,
        cache_read: u64,
        cache_creation: u64,
        calls: u64,
    ) -> UsageTracking {
        UsageTracking {
            total_input_tokens: input,
            total_output_tokens: output,
            total_cache_read_tokens: cache_read,
            total_cache_creation_tokens: cache_creation,
            total_reasoning_output_tokens: 0,
            total_cost_usd: 0.0,
            api_call_count: calls,
        }
    }

    #[test]
    fn context_usage_uses_the_latest_cumulative_delta_not_the_session_total() {
        let mut app = App::new();
        app.set_model_name("test-model".to_string());
        let settings = allthecodes_config::runtime_settings::SettingsJson {
            context_window: Some(1_000),
            ..Default::default()
        };
        app.set_context_capacity_from_settings(&settings);

        app.update_context_window_from_usage(&usage(100, 25, 10, 5, 1));
        assert_eq!(
            app.context_usage_summary()
                .expect("first context snapshot")
                .0,
            "140/1.0k (14%)"
        );

        app.update_context_window_from_usage(&usage(300, 75, 20, 5, 2));
        assert_eq!(
            app.context_usage_summary()
                .expect("second context snapshot")
                .0,
            "260/1.0k (26%)"
        );
    }

    #[test]
    fn context_capacity_applies_model_percent_and_unknown_models_stay_unknown() {
        let mut app = App::new();
        app.set_model_name("limited".to_string());
        let mut settings = allthecodes_config::runtime_settings::SettingsJson::default();
        settings.model_capabilities.insert(
            "limited".to_string(),
            allthecodes_config::settings::ModelCapabilitySettings {
                context_window: Some(2_000),
                effective_context_window_percent: Some(50),
                ..Default::default()
            },
        );
        app.set_context_capacity_from_settings(&settings);
        app.update_context_window_from_usage(&usage(800, 200, 0, 0, 1));
        assert_eq!(
            app.context_usage_summary()
                .expect("limited context snapshot")
                .0,
            "1.0k/1.0k (100%)"
        );

        app.set_model_name("not-configured".to_string());
        app.set_context_capacity_from_settings(&settings);
        app.update_context_window_from_usage(&usage(900, 300, 0, 0, 2));
        assert_eq!(
            app.context_usage_summary()
                .expect("unknown context snapshot")
                .0,
            "200/unknown"
        );
    }

    #[test]
    fn compact_boundary_replaces_stale_usage_with_post_compact_estimate() {
        let mut app = App::new();
        app.set_model_name("test-model".to_string());
        let settings = allthecodes_config::runtime_settings::SettingsJson {
            context_window: Some(1_000),
            ..Default::default()
        };
        app.set_context_capacity_from_settings(&settings);
        app.update_context_window_from_usage(&usage(700, 100, 0, 0, 1));
        assert_eq!(
            app.context_usage_summary().expect("pre-compact snapshot").0,
            "800/1.0k (80%)"
        );

        app.mark_context_compacted(Some(240));
        assert_eq!(
            app.context_usage_summary()
                .expect("post-compact snapshot")
                .0,
            "240/1.0k (24%)"
        );
    }

    #[test]
    fn over_capacity_context_is_explicitly_marked() {
        let snapshot = ContextWindowSnapshot {
            model_id: "test-model".to_string(),
            used_tokens: 1_500,
            effective_capacity: Some(1_000),
            source: ContextWindowSource::Estimated,
            phase: ContextWindowPhase::Completed,
        };
        assert_eq!(format_context_usage(&snapshot), "1.5k/1.0k (>150%)");
    }
}
