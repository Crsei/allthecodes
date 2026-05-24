//! Memory update notification rendering.

use std::path::Path;

use super::memory_file_selector::relative_memory_path;

pub fn render_memory_update_notification(memory_path: &Path, cwd: &Path, home: &Path) -> String {
    format!(
        "Memory updated in {} - /memory to edit",
        relative_memory_path(memory_path, cwd, home)
    )
}
