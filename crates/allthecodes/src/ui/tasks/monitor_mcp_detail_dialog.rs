//! MCP monitor task detail dialog.

use super::task_status_utils::task_header;
use super::TaskStatus;

pub fn render_monitor_mcp_detail_dialog(task: &TaskStatus, server_name: &str) -> String {
    format!(
        "{}\nserver: {}\n{}",
        task_header(task),
        server_name,
        task.output_lines.join("\n")
    )
}
