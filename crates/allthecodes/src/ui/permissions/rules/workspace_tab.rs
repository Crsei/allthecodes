//! Workspace permission tab rendering.

use super::WorkspaceDirectory;

pub fn render_workspace_tab(directories: &[WorkspaceDirectory], selected_index: usize) -> String {
    let mut lines = vec![format!("Workspace directories ({})", directories.len())];
    if directories.is_empty() {
        lines.push("  <none>".to_string());
    } else {
        lines.extend(directories.iter().enumerate().map(|(idx, directory)| {
            let marker = if idx == selected_index.min(directories.len().saturating_sub(1)) {
                ">"
            } else {
                " "
            };
            let trust = if directory.trusted {
                "trusted"
            } else {
                "untrusted"
            };
            format!("{marker} {} ({trust})", directory.path)
        }));
    }
    lines.join("\n")
}
