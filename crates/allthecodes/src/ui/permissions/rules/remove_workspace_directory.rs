//! Remove workspace directory rendering.

pub fn render_remove_workspace_directory(path: &str, dependent_rules: usize) -> String {
    format!("Remove workspace directory\npath: {path}\ndependent rules: {dependent_rules}")
}
