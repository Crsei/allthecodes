use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::command_surface::adapters::remote::{
    remote_surface_initial_snapshot, truncate_middle, RemoteSurfaceSnapshot,
};
use crate::ui::command_surface::{cycle_index, render_tabs, CommandSurfaceOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSurface {
    pub(crate) snapshot: RemoteSurfaceSnapshot,
    pub(crate) tab_index: usize,
    pub(crate) selected_index: usize,
}

impl RemoteSurface {
    pub(crate) fn new() -> Self {
        Self {
            snapshot: remote_surface_initial_snapshot(),
            tab_index: 0,
            selected_index: 0,
        }
    }

    pub(crate) fn render(&self) -> String {
        let mut lines = vec![
            "Remote control gateway".to_string(),
            render_tabs(&["Status", "Adapters", "Runs", "Security"], self.tab_index),
        ];
        match self.tab_index {
            0 => self.render_status(&mut lines),
            1 => self.render_adapters(&mut lines),
            2 => self.render_runs(&mut lines),
            _ => self.render_security(&mut lines),
        }
        lines.push(self.help_line());
        lines.join("\n")
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        match key.code {
            KeyCode::Left => {
                self.tab_index = cycle_index(self.tab_index, 4, -1);
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Right | KeyCode::Tab => {
                self.tab_index = cycle_index(self.tab_index, 4, 1);
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Char(ch) if ('1'..='4').contains(&ch) => {
                self.tab_index = (ch as usize) - ('1' as usize);
                self.selected_index = 0;
                CommandSurfaceOutcome::None
            }
            KeyCode::Up => {
                self.selected_index = cycle_index(self.selected_index, self.row_count(), -1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected_index = cycle_index(self.selected_index, self.row_count(), 1);
                CommandSurfaceOutcome::None
            }
            KeyCode::Enter => self.primary_action(),
            KeyCode::Char('r') => CommandSurfaceOutcome::Submit("/remote".to_string()),
            KeyCode::Char('c') if self.tab_index == 1 => self.adapter_command("connect"),
            KeyCode::Char('t') if self.tab_index == 1 => self.adapter_command("test-message"),
            KeyCode::Char('s') if self.tab_index == 2 => self.run_command("show"),
            KeyCode::Char('e') if self.tab_index == 2 => self.run_command("events"),
            KeyCode::Char('k') if self.tab_index == 2 => self.run_command("stop"),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn render_status(&self, lines: &mut Vec<String>) {
        lines.push(format!(
            "{} daemon: {}",
            marker(self.selected_index == 0),
            self.snapshot.daemon.state
        ));
        lines.push(format!("  {}", self.snapshot.daemon.detail));
        lines.push(format!("  enter: {}", self.snapshot.daemon.action));
    }

    fn render_adapters(&self, lines: &mut Vec<String>) {
        if self.snapshot.adapters.is_empty() {
            lines.push("No remote adapters registered.".to_string());
            return;
        }
        for (idx, adapter) in self.snapshot.adapters.iter().enumerate() {
            lines.push(format!(
                "{} {} - {}",
                marker(idx == self.selected_index),
                adapter.provider,
                adapter.state
            ));
            lines.push(format!("  enter: {}", adapter.action));
        }
    }

    fn render_runs(&self, lines: &mut Vec<String>) {
        if self.snapshot.runs.is_empty() {
            lines.push("No local gateway runs found.".to_string());
            lines.push("Run /remote runs to refresh the durable store view.".to_string());
            return;
        }
        for (idx, run) in self.snapshot.runs.iter().enumerate() {
            lines.push(format!(
                "{} {} - {}",
                marker(idx == self.selected_index),
                truncate_middle(&run.run_id, 26),
                run.status
            ));
            lines.push(format!(
                "  source={} updated={}",
                run.source, run.updated_at_ms
            ));
        }
    }

    fn render_security(&self, lines: &mut Vec<String>) {
        for (idx, row) in self.snapshot.security.iter().enumerate() {
            lines.push(format!(
                "{} {}: {}",
                marker(idx == self.selected_index),
                row.label,
                row.value
            ));
        }
    }

    fn help_line(&self) -> String {
        match self.tab_index {
            1 => "Left/Right tabs | Up/Down select | Enter connect | c connect | t test | r refresh | Esc close".to_string(),
            2 => "Left/Right tabs | Up/Down select | Enter show | e events | k stop | r refresh | Esc close".to_string(),
            _ => "Left/Right tabs | Up/Down select | Enter action | r refresh | Esc close".to_string(),
        }
    }

    fn row_count(&self) -> usize {
        match self.tab_index {
            0 => 1,
            1 => self.snapshot.adapters.len(),
            2 => self.snapshot.runs.len(),
            _ => self.snapshot.security.len(),
        }
    }

    fn primary_action(&self) -> CommandSurfaceOutcome {
        match self.tab_index {
            0 => CommandSurfaceOutcome::Submit(self.snapshot.daemon.action.clone()),
            1 => self.adapter_command("connect"),
            2 => self.run_command("show"),
            _ => CommandSurfaceOutcome::Submit("/remote doctor".to_string()),
        }
    }

    fn adapter_command(&self, action: &str) -> CommandSurfaceOutcome {
        let Some(adapter) = self.snapshot.adapters.get(self.selected_index) else {
            return CommandSurfaceOutcome::None;
        };
        match action {
            "connect" => {
                CommandSurfaceOutcome::Submit(format!("/remote connect {}", adapter.provider))
            }
            "test-message" => CommandSurfaceOutcome::FillPrompt(format!(
                "/remote test-message {} ",
                adapter.provider
            )),
            _ => CommandSurfaceOutcome::None,
        }
    }

    fn run_command(&self, action: &str) -> CommandSurfaceOutcome {
        let Some(run) = self.snapshot.runs.get(self.selected_index) else {
            return CommandSurfaceOutcome::None;
        };
        CommandSurfaceOutcome::Submit(format!("/remote {action} {}", run.run_id))
    }
}

fn marker(selected: bool) -> &'static str {
    if selected {
        ">"
    } else {
        " "
    }
}

#[cfg(test)]
mod tests {
    use allthecodes_gateway::{RemoteSource, RemoteTransport, RunPolicy, RunRequest};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    use serial_test::serial;
    use std::path::Path;

    use super::*;
    use crate::ui::command_surface::adapters::remote::{
        RemoteAdapterRow, RemoteDaemonRow, RemoteRunRow, RemoteSecurityRow,
    };

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn snapshot() -> RemoteSurfaceSnapshot {
        RemoteSurfaceSnapshot {
            daemon: RemoteDaemonRow {
                state: "running".into(),
                detail: "pid=1; base=http://127.0.0.1:1234".into(),
                action: "/remote status".into(),
            },
            adapters: vec![RemoteAdapterRow {
                provider: "telegram".into(),
                state: "status via /remote adapters".into(),
                action: "/remote connect telegram".into(),
            }],
            runs: vec![RemoteRunRow {
                run_id: "run_1234567890".into(),
                status: "running".into(),
                source: "local:thread".into(),
                updated_at_ms: 42,
            }],
            security: vec![RemoteSecurityRow {
                label: "auth".into(),
                value: "local daemon token required; raw value hidden".into(),
            }],
        }
    }

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
            if let Some(previous) = &self.previous {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn remote_surface_renders_status_without_network_io() {
        let surface = RemoteSurface {
            snapshot: snapshot(),
            tab_index: 0,
            selected_index: 0,
        };

        assert!(surface.render().contains("Remote control gateway"));
        assert!(surface.render().contains("[Status]"));
        assert!(surface.render().contains("/remote status"));
    }

    #[test]
    #[serial]
    fn remote_surface_new_uses_nonblocking_placeholder_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = EnvGuard::set("ALLTHECODES_HOME", temp.path());
        let store = allthecodes_gateway::GatewayStore::default_with_policy(
            allthecodes_gateway::SessionKeyPolicy::default(),
        );
        store.create_run(request()).unwrap();

        let surface = RemoteSurface::new();

        assert_eq!(surface.snapshot.daemon.state, "unknown");
        assert!(surface.snapshot.runs.is_empty());
        assert!(surface.render().contains("/remote status"));
    }

    #[test]
    fn remote_surface_routes_adapter_and_run_actions() {
        let mut surface = RemoteSurface {
            snapshot: snapshot(),
            tab_index: 1,
            selected_index: 0,
        };

        assert_eq!(
            surface.handle_key(key(KeyCode::Char('t'))),
            CommandSurfaceOutcome::FillPrompt("/remote test-message telegram ".to_string())
        );
        surface.handle_key(key(KeyCode::Right));
        assert_eq!(
            surface.handle_key(key(KeyCode::Enter)),
            CommandSurfaceOutcome::Submit("/remote show run_1234567890".to_string())
        );
        assert_eq!(
            surface.handle_key(key(KeyCode::Char('k'))),
            CommandSurfaceOutcome::Submit("/remote stop run_1234567890".to_string())
        );
    }

    fn request() -> RunRequest {
        RunRequest {
            prompt: "hello from remote surface".to_string(),
            source: RemoteSource::new(
                RemoteTransport::Local,
                "local",
                "workspace",
                "remote-surface-test",
                "user",
                "thread",
            ),
            policy: RunPolicy::default(),
            idempotency_key: None,
        }
    }
}
