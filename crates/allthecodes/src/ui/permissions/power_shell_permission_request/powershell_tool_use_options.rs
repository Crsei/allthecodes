//! PowerShell permission options.

use crate::ui::permissions::shell_permission_helpers::shell_permission_options;
use crate::ui::permissions::utils::render_permission_options;

pub fn render_powershell_tool_use_options(command: &str, selected_index: usize) -> String {
    render_permission_options(&shell_permission_options(command), selected_index).join("\n")
}
