use std::collections::{BTreeMap, BTreeSet};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ui::better_view_panel::BetterViewPanel;
use crate::ui::command_surface::{CommandSurfaceCursorAnchor, CommandSurfaceOutcome};
use crate::ui::selection_surface::{SelectionItem, SelectionSurface, SelectionSurfaceEvent};
use allthecodes_plugins::{PluginEntry, PluginSource, PluginStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginSurface {
    pub(crate) picker: SelectionSurface,
}

impl PluginSurface {
    pub(crate) fn new() -> Self {
        Self {
            picker: build_plugin_picker(),
        }
    }

    pub(crate) fn render(&self) -> String {
        BetterViewPanel::new("Plugins")
            .summary("Installed, enabled, active, and error state")
            .detail_title("Plugin selector")
            .detail_lines(self.picker.render_lines(14))
            .footer("Enter info | Ctrl+S search | e enable | d disable | u uninstall | r reload | Esc close")
            .render()
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> CommandSurfaceOutcome {
        if key.kind != KeyEventKind::Press {
            return CommandSurfaceOutcome::None;
        }

        match key.code {
            KeyCode::Char('s')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && !self.picker.filter.trim().is_empty() =>
            {
                CommandSurfaceOutcome::Submit(format!(
                    "/plugin search {}",
                    self.picker.filter.trim()
                ))
            }
            KeyCode::Char('e') => self.selected_plugin_command("enable"),
            KeyCode::Char('d') => self.selected_plugin_command("disable"),
            KeyCode::Char('u') => self
                .selected_plugin_id()
                .map(|id| CommandSurfaceOutcome::FillPrompt(format!("/plugin uninstall {id} ")))
                .unwrap_or(CommandSurfaceOutcome::None),
            KeyCode::Char('r') => CommandSurfaceOutcome::Submit("/reload-plugins".to_string()),
            _ => match self.picker.handle_key(key) {
                SelectionSurfaceEvent::Selected(id) => {
                    CommandSurfaceOutcome::Submit(format!("/plugin info {id}"))
                }
                SelectionSurfaceEvent::Closed => CommandSurfaceOutcome::Close,
                SelectionSurfaceEvent::None => CommandSurfaceOutcome::None,
            },
        }
    }

    pub(crate) fn cursor_anchor(&self) -> Option<CommandSurfaceCursorAnchor> {
        Some(CommandSurfaceCursorAnchor::Search {
            marker: format!("{} / ", self.picker.title),
            value: self.picker.filter.clone(),
        })
    }

    fn selected_plugin_id(&self) -> Option<String> {
        self.picker.selected_item().map(|item| item.id.clone())
    }

    fn selected_plugin_command(&self, action: &str) -> CommandSurfaceOutcome {
        self.selected_plugin_id()
            .map(|id| CommandSurfaceOutcome::Submit(format!("/plugin {action} {id}")))
            .unwrap_or(CommandSurfaceOutcome::None)
    }
}

fn build_plugin_picker() -> SelectionSurface {
    let installed = allthecodes_plugins::loader::load_installed_plugins();
    let active = allthecodes_plugins::get_all_plugins();
    let active_ids = active
        .iter()
        .map(|plugin| plugin.id.clone())
        .collect::<BTreeSet<_>>();

    let mut merged = BTreeMap::<String, PluginEntry>::new();
    for plugin in active {
        merged.insert(plugin.id.clone(), plugin);
    }
    for plugin in installed {
        merged.insert(plugin.id.clone(), plugin);
    }

    let items = if merged.is_empty() {
        vec![SelectionItem {
            id: "status".to_string(),
            label: "No installed plugins".to_string(),
            description: "use /plugin install <source>".to_string(),
            enabled: false,
            disabled_reason: Some("no plugins found".to_string()),
            preview_lines: vec![
                "Plugin metadata is loaded from ~/.allthecodes/plugins/installed_plugins.json."
                    .to_string(),
                "Press r to reload after installing or editing plugin files.".to_string(),
            ],
            actions: Vec::new(),
            search_terms: Vec::new(),
        }]
    } else {
        merged
            .into_values()
            .map(|plugin| {
                let active = active_ids.contains(&plugin.id);
                plugin_item(plugin, active)
            })
            .collect()
    };

    SelectionSurface::new("Plugins", items)
}

fn plugin_item(plugin: PluginEntry, active: bool) -> SelectionItem {
    let installed = !matches!(plugin.status, PluginStatus::NotInstalled);
    let enabled = matches!(plugin.status, PluginStatus::Installed);
    let status = status_label(&plugin.status);
    let mut preview_lines = vec![
        format!("ID: {}", plugin.id),
        format!("Version: {}", plugin.version),
        format!("Source: {}", source_label(&plugin.source)),
        format!("Installed: {}", yes_no(installed)),
        format!("Enabled: {}", yes_no(enabled)),
        format!("Active: {}", yes_no(active)),
    ];
    if let PluginStatus::Error(error) = &plugin.status {
        preview_lines.push(format!("Error: {error}"));
    }
    if !plugin.tools.is_empty() {
        preview_lines.push(format!("Tools: {}", plugin.tools.join(", ")));
    }
    if !plugin.skills.is_empty() {
        preview_lines.push(format!("Skills: {}", plugin.skills.join(", ")));
    }
    if !plugin.mcp_servers.is_empty() {
        preview_lines.push(format!("MCP servers: {}", plugin.mcp_servers.join(", ")));
    }

    SelectionItem {
        id: plugin.id.clone(),
        label: format!("{} ({})", plugin.name, plugin.id),
        description: format!(
            "installed={} enabled={} active={} status={}",
            yes_no(installed),
            yes_no(enabled),
            yes_no(active),
            status,
        ),
        enabled: installed,
        disabled_reason: (!installed).then(|| "not installed".to_string()),
        preview_lines,
        actions: Vec::new(),
        search_terms: vec![plugin.name, plugin.description, status.to_string()],
    }
}

fn status_label(status: &PluginStatus) -> &'static str {
    match status {
        PluginStatus::NotInstalled => "not-installed",
        PluginStatus::Installed => "installed",
        PluginStatus::Disabled => "disabled",
        PluginStatus::Error(_) => "error",
    }
}

fn source_label(source: &PluginSource) -> String {
    match source {
        PluginSource::Npm { package, version } => version
            .as_ref()
            .map(|version| format!("npm:{package}@{version}"))
            .unwrap_or_else(|| format!("npm:{package}")),
        PluginSource::GitHub { repo, ref_spec } => ref_spec
            .as_ref()
            .map(|ref_spec| format!("github:{repo}#{ref_spec}"))
            .unwrap_or_else(|| format!("github:{repo}")),
        PluginSource::Git { url, ref_spec } => ref_spec
            .as_ref()
            .map(|ref_spec| format!("git:{url}#{ref_spec}"))
            .unwrap_or_else(|| format!("git:{url}")),
        PluginSource::Url { url } => url.clone(),
        PluginSource::Marketplace { id, source_name } => format!("marketplace:{source_name}/{id}"),
        PluginSource::Local { path } => path.clone(),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
