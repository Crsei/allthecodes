use allthecodes_types::kairos::{
    KairosFeatureProfile, KairosLifecycleState, KairosRuntimeSnapshot,
};
use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::command_surface::adapters::kairos::{latest_kairos_snapshot, KairosSurfaceSnapshot};
use crate::ui::command_surface::{cycle_index, render_tabs, CommandSurfaceOutcome};

const TABS: [&str; 4] = ["Overview", "Features", "Workers", "Bridge"];
const FEATURES: [&str; 5] = [
    "brief",
    "channels",
    "push-notifications",
    "github-webhooks",
    "proactive",
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KairosSurface {
    tab_index: usize,
    selected_index: usize,
}

impl KairosSurface {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn render(&self) -> String {
        let cached = latest_kairos_snapshot();
        let mut lines = vec![
            "KAIROS local control plane".to_string(),
            render_tabs(&TABS, self.tab_index),
            String::new(),
        ];
        match self.tab_index {
            0 => self.render_overview(&cached, &mut lines),
            1 => self.render_features(&cached, &mut lines),
            2 => self.render_workers(&cached, &mut lines),
            _ => self.render_bridge(&mut lines),
        }
        lines.push(String::new());
        lines.push(self.help_line().to_string());
        lines.join("\n")
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left | KeyCode::BackTab => {
                self.tab_index = cycle_index(self.tab_index, TABS.len(), -1);
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Tab => {
                self.tab_index = cycle_index(self.tab_index, TABS.len(), 1);
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected_index = cycle_index(self.selected_index, self.row_count(), -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_index = cycle_index(self.selected_index, self.row_count(), 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self.primary_action(),
            KeyCode::Char('a') => CommandSurfaceOutcome::Submit("/kairos reconcile".to_string()),
            KeyCode::Char('r') if self.tab_index == 0 => {
                CommandSurfaceOutcome::Submit("/kairos restart".to_string())
            }
            KeyCode::Char('r') => CommandSurfaceOutcome::None,
            KeyCode::Char('s') => CommandSurfaceOutcome::Submit("/kairos stop".to_string()),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn render_overview(&self, cached: &KairosSurfaceSnapshot, lines: &mut Vec<String>) {
        let Some(snapshot) = cached.runtime.as_ref() else {
            lines.push("Lifecycle: unknown".to_string());
            lines.push(format!(
                "Error: {}",
                cached.error.as_deref().unwrap_or("waiting for first poll")
            ));
            lines.push("> Refresh (polling automatically)".to_string());
            return;
        };
        lines.push(format!("Lifecycle: {:?}", snapshot.lifecycle));
        lines.push(format!("Desired:   {}", profile_summary(&snapshot.desired)));
        lines.push(format!(
            "Effective: {}",
            profile_summary(&snapshot.effective)
        ));
        lines.push(format!(
            "Running:   {}",
            snapshot
                .running
                .as_ref()
                .map(profile_summary)
                .unwrap_or_else(|| "none".to_string())
        ));
        if let Some(supervisor) = &snapshot.supervisor {
            lines.push(format!(
                "Supervisor: pid={} {}",
                supervisor.pid, supervisor.health_url
            ));
        }
        lines.push(format!("Restart required: {}", snapshot.restart_required));
        if let Some(transition) = &snapshot.last_transition {
            lines.push(format!(
                "Last: {:?} -> {:?} at {}",
                transition.from, transition.to, transition.timestamp
            ));
            if let Some(message) = &transition.message {
                lines.push(format!("  {message}"));
            }
        }
        lines.push(format!("> {}", primary_label(snapshot)));
    }

    fn render_features(&self, cached: &KairosSurfaceSnapshot, lines: &mut Vec<String>) {
        let Some(snapshot) = cached.runtime.as_ref() else {
            lines.push("Feature state unavailable until the first successful poll.".to_string());
            return;
        };
        for (index, feature) in FEATURES.iter().enumerate() {
            let desired = feature_value(&snapshot.desired, feature);
            let effective = feature_value(&snapshot.effective, feature);
            let running = snapshot
                .running
                .as_ref()
                .map(|profile| feature_value(profile, feature));
            let source = feature_source(snapshot, feature);
            lines.push(format!(
                "{} {:<20} desired={} effective={} running={} source={source}",
                if index == self.selected_index {
                    ">"
                } else {
                    " "
                },
                feature,
                on_off(desired),
                on_off(effective),
                running.map(on_off).unwrap_or("-")
            ));
        }
        for diagnostic in &snapshot.diagnostics {
            lines.push(format!(
                "blocked: {} - {}",
                diagnostic.code, diagnostic.message
            ));
        }
    }

    fn render_workers(&self, cached: &KairosSurfaceSnapshot, lines: &mut Vec<String>) {
        let Some(snapshot) = cached.runtime.as_ref() else {
            lines.push("Worker state unavailable.".to_string());
            return;
        };
        if snapshot.workers.is_empty() {
            lines.push(match snapshot.lifecycle {
                KairosLifecycleState::Stopped => "Workers: daemon stopped".to_string(),
                KairosLifecycleState::Failed => "Workers: unavailable after failure".to_string(),
                _ => "Workers: none reported".to_string(),
            });
        }
        for worker in &snapshot.workers {
            lines.push(format!(
                "{} kind={} pid={} status={} restarts={} updated={}",
                worker.worker_id,
                worker.kind,
                worker
                    .pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "-".into()),
                worker.status,
                worker.restart_count,
                worker.updated_at.as_deref().unwrap_or("-")
            ));
        }
        if let Some(automation) = &snapshot.automation {
            lines.push(format!(
                "Automation: {} proactive={} next={} sleep={} reason={}",
                automation.status,
                automation.proactive_active,
                automation.next_tick_at.as_deref().unwrap_or("-"),
                automation.sleeping_until.as_deref().unwrap_or("-"),
                automation.reason.as_deref().unwrap_or("-")
            ));
        }
    }

    fn render_bridge(&self, lines: &mut Vec<String>) {
        lines
            .push("Bridge leases are explicit and are not attached by Enable & Start.".to_string());
        lines.push("> List bridge sessions".to_string());
        lines.push("Use /kairos bridge resume <id>, new, or release <id>.".to_string());
    }

    fn primary_action(&self) -> CommandSurfaceOutcome {
        if self.tab_index == 1 {
            let Some(feature) = FEATURES.get(self.selected_index) else {
                return CommandSurfaceOutcome::None;
            };
            let cached = latest_kairos_snapshot();
            let current = cached
                .runtime
                .as_ref()
                .map(|snapshot| feature_value(&snapshot.desired, feature))
                .unwrap_or(false);
            return CommandSurfaceOutcome::Submit(format!(
                "/kairos feature {feature} {}",
                if current { "off" } else { "on" }
            ));
        }
        if self.tab_index == 3 {
            return CommandSurfaceOutcome::Submit("/kairos bridge sessions".to_string());
        }
        if self.tab_index == 2 {
            return CommandSurfaceOutcome::None;
        }
        let cached = latest_kairos_snapshot();
        let Some(snapshot) = cached.runtime.as_ref() else {
            return CommandSurfaceOutcome::None;
        };
        let command = match snapshot.lifecycle {
            KairosLifecycleState::Starting
            | KairosLifecycleState::Stopping
            | KairosLifecycleState::Restarting => return CommandSurfaceOutcome::None,
            KairosLifecycleState::Ready if snapshot.restart_required => "/kairos restart",
            KairosLifecycleState::Ready => "/kairos stop",
            _ if !snapshot.desired.enabled => "/kairos enable --scope local --start",
            _ => "/kairos start",
        };
        CommandSurfaceOutcome::Submit(command.to_string())
    }

    fn row_count(&self) -> usize {
        match self.tab_index {
            1 => FEATURES.len(),
            _ => 1,
        }
    }

    fn help_line(&self) -> &'static str {
        match self.tab_index {
            1 => "Left/Right tabs | Up/Down feature | Enter toggle | a apply | Esc close",
            2 => "Left/Right tabs | s stop | Esc close",
            3 => "Left/Right tabs | Enter list sessions | Esc close",
            _ => "Left/Right tabs | Enter primary action | r restart | s stop | Esc close",
        }
    }
}

fn primary_label(snapshot: &KairosRuntimeSnapshot) -> &'static str {
    match snapshot.lifecycle {
        KairosLifecycleState::Starting => "Starting...",
        KairosLifecycleState::Stopping => "Stopping...",
        KairosLifecycleState::Restarting => "Restarting...",
        KairosLifecycleState::Ready if snapshot.restart_required => "Restart to apply",
        KairosLifecycleState::Ready => "Stop",
        KairosLifecycleState::Failed | KairosLifecycleState::Stale => "Repair & Start",
        _ if snapshot.desired.enabled => "Start",
        _ => "Enable & Start",
    }
}

fn profile_summary(profile: &KairosFeatureProfile) -> String {
    format!(
        "enabled={} brief={} channels={} push={} github={} proactive={}",
        profile.enabled,
        profile.brief,
        profile.channels,
        profile.push_notifications,
        profile.github_webhooks,
        profile.proactive
    )
}

fn feature_value(profile: &KairosFeatureProfile, feature: &str) -> bool {
    match feature {
        "brief" => profile.brief,
        "channels" => profile.channels,
        "push-notifications" => profile.push_notifications,
        "github-webhooks" => profile.github_webhooks,
        "proactive" => profile.proactive,
        _ => false,
    }
}

fn feature_source(snapshot: &KairosRuntimeSnapshot, feature: &str) -> String {
    format!(
        "{:?}",
        match feature {
            "brief" => snapshot.sources.brief,
            "channels" => snapshot.sources.channels,
            "push-notifications" => snapshot.sources.push_notifications,
            "github-webhooks" => snapshot.sources.github_webhooks,
            "proactive" => snapshot.sources.proactive,
            _ => snapshot.sources.enabled,
        }
    )
}

fn on_off(value: bool) -> &'static str {
    if value {
        "on"
    } else {
        "off"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::command_surface::adapters::kairos::set_kairos_snapshot;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    #[serial_test::serial]
    fn disabled_overview_offers_enable_and_start() {
        set_kairos_snapshot(Ok(KairosRuntimeSnapshot::default()));
        let surface = KairosSurface::new();
        assert!(surface.render().contains("Enable & Start"));
        assert_eq!(
            surface.clone().handle_key(key(KeyCode::Enter)),
            CommandSurfaceOutcome::Submit("/kairos enable --scope local --start".to_string())
        );
    }

    #[test]
    #[serial_test::serial]
    fn feature_toggle_preserves_explicit_off_action() {
        let snapshot = KairosRuntimeSnapshot {
            desired: KairosFeatureProfile {
                enabled: true,
                brief: true,
                ..Default::default()
            },
            ..Default::default()
        };
        set_kairos_snapshot(Ok(snapshot));
        let mut surface = KairosSurface::new();
        surface.handle_key(key(KeyCode::Right));
        assert_eq!(
            surface.handle_key(key(KeyCode::Enter)),
            CommandSurfaceOutcome::Submit("/kairos feature brief off".to_string())
        );
    }
}
