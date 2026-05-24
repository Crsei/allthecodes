//! Notebook edit diff rendering.

pub fn render_notebook_edit_tool_diff(path: &str, cell_index: usize, language: &str) -> String {
    format!("notebook: {path}\ncell: {cell_index}\nlanguage: {language}")
}
