//! Add workspace directory rendering.

use super::WorkspaceDirectory;

pub fn render_add_workspace_directory(path: &str, trusted: bool) -> String {
    let directory = WorkspaceDirectory {
        path: path.to_string(),
        trusted,
    };
    format!(
        "Add workspace directory\npath: {}\ntrusted: {}",
        directory.path, directory.trusted
    )
}
