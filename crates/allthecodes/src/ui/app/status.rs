use crate::ui::notifications::in_app::{InAppNotification, NotificationPriority, NotificationTone};
use crate::ui::status_line::{
    build_payload_from_snapshot, payload, StatusLinePayload, StatusLineRunner, StatusLineSnapshot,
};
use allthecodes_config::settings::StatusLineSettings;

use super::App;
use super::GoalStatusSnapshot;
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
        let effort = state
            .effort_value
            .clone()
            .or_else(|| output_config_effort(state.settings.output_config.as_ref()))
            .or_else(|| state.settings.effort_level.clone())
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
            self.vscroll.invalidate_all();
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
            session_id: (!self.session_id.is_empty()).then(|| self.session_id.clone()),
            model_id: &self.model_name,
            backend: (!self.backend_name.is_empty()).then_some(self.backend_name.as_str()),
            cwd: std::path::Path::new(&self.cwd),
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
                self.output_style.as_deref(),
                std::path::Path::new(&self.cwd),
            ),
            editor_mode: self.vim.enabled.then_some("vim"),
            worktree: crate::ui::status_line_resolver::current_worktree_status_for_session(
                (!self.session_id.is_empty()).then_some(self.session_id.as_str()),
            ),
            streaming: self.is_streaming,
            message_count: self.messages.len(),
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

fn output_config_effort(output_config: Option<&serde_json::Value>) -> Option<String> {
    output_config?
        .get("effort")
        .and_then(allthecodes_engine::effort::normalize_output_effort_json)
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
