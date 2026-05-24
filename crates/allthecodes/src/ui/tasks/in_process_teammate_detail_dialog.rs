//! In-process teammate detail dialog.

use super::task_status_utils::task_header;
use super::TaskStatus;

pub fn render_in_process_teammate_detail_dialog(task: &TaskStatus, teammate_name: &str) -> String {
    format!(
        "{}\nteammate: {}\n{}",
        task_header(task),
        teammate_name,
        task.summary
    )
}
