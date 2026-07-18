// BEGIN generated upstream messages modules
// Rust-side message modules mirrored from upstream React components.
#[path = "messages/advisor_message.rs"]
pub mod advisor_message;
#[path = "messages/assistant_redacted_thinking_message.rs"]
pub mod assistant_redacted_thinking_message;
#[path = "messages/assistant_text_message.rs"]
pub mod assistant_text_message;
#[path = "messages/assistant_thinking_message.rs"]
pub mod assistant_thinking_message;
#[path = "messages/assistant_tool_use_message.rs"]
pub mod assistant_tool_use_message;
#[path = "messages/attachment_message.rs"]
pub mod attachment_message;
#[path = "messages/collapsed_read_search_content.rs"]
pub mod collapsed_read_search_content;
#[path = "messages/compact_boundary_message.rs"]
pub mod compact_boundary_message;
#[path = "messages/file_edit_tool_updated_message.rs"]
pub mod file_edit_tool_updated_message;
#[path = "messages/grouped_tool_use_content.rs"]
pub mod grouped_tool_use_content;
#[path = "messages/highlighted_thinking_text.rs"]
pub mod highlighted_thinking_text;
#[path = "messages/hook_progress_message.rs"]
pub mod hook_progress_message;
#[path = "messages/null_rendering_attachments.rs"]
pub mod null_rendering_attachments;
#[path = "messages/plan_approval_message.rs"]
pub mod plan_approval_message;
#[path = "messages/rate_limit_message.rs"]
pub mod rate_limit_message;
#[path = "messages/shutdown_message.rs"]
pub mod shutdown_message;
#[path = "messages/system_api_error_message.rs"]
pub mod system_api_error_message;
#[path = "messages/system_text_message.rs"]
pub mod system_text_message;
#[path = "messages/task_assignment_message.rs"]
pub mod task_assignment_message;
#[path = "messages/task_list_content.rs"]
// This shared renderer is wired into the Spinner by the following
// tui-tool-render-style-upgrade worktree.
#[allow(dead_code)]
pub mod task_list_content;
#[path = "messages/team_mem_collapsed.rs"]
pub mod team_mem_collapsed;
#[path = "messages/team_mem_saved.rs"]
pub mod team_mem_saved;
#[path = "messages/tool_operation_content.rs"]
pub mod tool_operation_content;
#[path = "messages/user_agent_notification_message.rs"]
pub mod user_agent_notification_message;
#[path = "messages/user_bash_input_message.rs"]
pub mod user_bash_input_message;
#[path = "messages/user_bash_output_message.rs"]
pub mod user_bash_output_message;
#[path = "messages/user_channel_message.rs"]
pub mod user_channel_message;
#[path = "messages/user_command_message.rs"]
pub mod user_command_message;
#[path = "messages/user_image_message.rs"]
pub mod user_image_message;
#[path = "messages/user_local_command_output_message.rs"]
pub mod user_local_command_output_message;
#[path = "messages/user_memory_input_message.rs"]
pub mod user_memory_input_message;
#[path = "messages/user_plan_message.rs"]
pub mod user_plan_message;
#[path = "messages/user_prompt_message.rs"]
pub mod user_prompt_message;
#[path = "messages/user_resource_update_message.rs"]
pub mod user_resource_update_message;
#[path = "messages/user_teammate_message.rs"]
pub mod user_teammate_message;
#[path = "messages/user_text_message.rs"]
pub mod user_text_message;
#[path = "messages/user_tool_result_message/mod.rs"]
pub mod user_tool_result_message;
// END generated upstream messages modules

#[path = "messages/render/mod.rs"]
mod render;
#[path = "messages/view_model.rs"]
pub mod view_model;
#[path = "messages/wrap.rs"]
mod wrap;

#[cfg(test)]
pub(super) use render::build_message_render_context;
pub use render::render_messages;
pub(super) use render::{
    message_copy_text, message_copy_text_with_mode, message_primary_reference,
    render_renderable_message_for_layout, CopyTextMode, MessageRenderContext, MessageRenderOptions,
};
pub(crate) use view_model::MessageListViewModel;

#[cfg(test)]
mod tests {
    use super::advisor_message::render_advisor_message;
    use super::assistant_redacted_thinking_message::render_assistant_redacted_thinking_message;
    use super::assistant_text_message::render_assistant_text_message;
    use super::assistant_thinking_message::render_assistant_thinking_message;
    use super::assistant_tool_use_message::{render_assistant_tool_use_message, ToolUseState};
    use super::attachment_message::render_attachment_message;
    use super::collapsed_read_search_content::render_collapsed_read_search_content;
    use super::compact_boundary_message::render_compact_boundary_message;
    use super::file_edit_tool_updated_message::{
        render_file_edit_tool_canceled_message, render_file_edit_tool_rejected_message,
        render_file_edit_tool_updated_message, FileEditMessageStyle, FileEditToolUpdatedView,
    };
    use super::grouped_tool_use_content::render_grouped_tool_use_content;
    use super::highlighted_thinking_text::render_highlighted_thinking_text;
    use super::hook_progress_message::render_hook_progress_message;
    use super::null_rendering_attachments::render_null_rendering_attachments;
    use super::plan_approval_message::render_plan_approval_message;
    use super::rate_limit_message::render_rate_limit_message;
    use super::shutdown_message::render_shutdown_message;
    use super::system_api_error_message::render_system_api_error_message;
    use super::system_text_message::render_system_text_message;
    use super::task_assignment_message::render_task_assignment_message;
    use super::team_mem_collapsed::render_team_mem_collapsed;
    use super::team_mem_saved::render_team_mem_saved;
    use super::user_agent_notification_message::render_user_agent_notification_message;
    use super::user_bash_input_message::render_user_bash_input_message;
    use super::user_bash_output_message::{
        render_user_bash_output_message, render_user_bash_output_message_with_options,
        ShellOutputRenderOptions,
    };
    use super::user_channel_message::render_user_channel_message;
    use super::user_command_message::render_user_command_message;
    use super::user_image_message::render_user_image_message;
    use super::user_local_command_output_message::render_user_local_command_output_message;
    use super::user_memory_input_message::render_user_memory_input_message;
    use super::user_plan_message::render_user_plan_message;
    use super::user_prompt_message::render_user_prompt_message;
    use super::user_resource_update_message::render_user_resource_update_message;
    use super::user_teammate_message::render_user_teammate_message;
    use super::user_text_message::render_user_text_message;
    use crate::ui::diff::file_edit_diff::unified_hunk_lines_from_edit;
    use crate::ui::theme::Theme;

    #[test]
    fn snapshot_message_component_helpers() {
        let theme = Theme::default();
        let edit_hunks = unified_hunk_lines_from_edit(
            "src/lib.rs",
            "fn main() {\n    old_call();\n}\n",
            "fn main() {\n    new_call();\n    extra_call();\n}\n",
        );
        let rendered = [
            section(
                "advisor",
                render_advisor_message(Some("gpt"), "Plan next steps", "No issues", &theme),
            ),
            section(
                "assistant-redacted-thinking",
                render_assistant_redacted_thinking_message(&theme),
            ),
            section(
                "assistant-text",
                render_assistant_text_message("Hello", &theme),
            ),
            section(
                "assistant-thinking",
                render_assistant_thinking_message("I will inspect the files", &theme),
            ),
            section(
                "assistant-tool-use",
                render_assistant_tool_use_message(
                    "read_file",
                    "path=src/main.rs",
                    ToolUseState::Resolved,
                    false,
                    &theme,
                ),
            ),
            section(
                "attachment",
                render_attachment_message("log", "appended", &theme),
            ),
            section(
                "collapsed-read-search",
                render_collapsed_read_search_content("notes.txt", 3, &theme),
            ),
            section(
                "compact-boundary",
                render_compact_boundary_message(120, 80, &theme),
            ),
            section(
                "file-edit-updated",
                render_file_edit_tool_updated_message(&FileEditToolUpdatedView {
                    file_path: "src/lib.rs".to_string(),
                    hunk_lines: edit_hunks.clone(),
                    style: FileEditMessageStyle::Regular,
                    verbose: true,
                    preview_hint: None,
                    width: 80,
                    max_lines: 20,
                }),
            ),
            section(
                "file-edit-condensed",
                render_file_edit_tool_updated_message(&FileEditToolUpdatedView {
                    file_path: "src/lib.rs".to_string(),
                    hunk_lines: edit_hunks,
                    style: FileEditMessageStyle::Condensed,
                    verbose: false,
                    preview_hint: None,
                    width: 80,
                    max_lines: 20,
                }),
            ),
            section(
                "file-edit-rejected",
                render_file_edit_tool_rejected_message("src/lib.rs", Some("needs review")),
            ),
            section(
                "file-edit-canceled",
                render_file_edit_tool_canceled_message("src/lib.rs"),
            ),
            section(
                "grouped-tool-use",
                render_grouped_tool_use_content(&["read_file", "edit_file"], &theme),
            ),
            section(
                "highlighted-thinking",
                render_highlighted_thinking_text("cache warmup", &theme),
            ),
            section(
                "hook-progress",
                render_hook_progress_message("PostToolUse", "running", &theme),
            ),
            section(
                "null-rendering-attachments",
                render_null_rendering_attachments("filtered", &theme),
            ),
            section(
                "plan-approval",
                render_plan_approval_message("refactor ui", true, &theme),
            ),
            section(
                "rate-limit",
                render_rate_limit_message("messages", 250, &theme),
            ),
            section(
                "shutdown",
                render_shutdown_message("user requested", &theme),
            ),
            section(
                "system-api-error",
                render_system_api_error_message(429, "rate limit exceeded", &theme),
            ),
            section(
                "system-text",
                render_system_text_message("note", "Context switch", &theme),
            ),
            section(
                "task-assignment",
                render_task_assignment_message("build", "alice", &theme),
            ),
            section(
                "team-mem-collapsed",
                render_team_mem_collapsed("team-a", 4, &theme),
            ),
            section(
                "team-mem-saved",
                render_team_mem_saved("/tmp/team.md", &theme),
            ),
            section(
                "user-agent-notification",
                render_user_agent_notification_message("Agent connected", &theme),
            ),
            section(
                "user-bash-input",
                render_user_bash_input_message("ls -la", &theme),
            ),
            section(
                "user-bash-output",
                render_user_bash_output_message("ls -la", "README.md", &theme),
            ),
            section(
                "user-bash-output-collapsed",
                render_user_bash_output_message_with_options(
                    "cargo test",
                    "one\ntwo\nthree\nfour\nfive\nsix",
                    ShellOutputRenderOptions {
                        width: 80,
                        elapsed_ms: Some(12_400),
                        total_bytes: Some(2048),
                        ..ShellOutputRenderOptions::default()
                    },
                ),
            ),
            section(
                "user-channel",
                render_user_channel_message("default", "status ok", &theme),
            ),
            section(
                "user-command",
                render_user_command_message("build", Some("/repo"), &theme),
            ),
            section(
                "user-image",
                render_user_image_message("/tmp/a.png", "png", &theme),
            ),
            section(
                "user-local-output",
                render_user_local_command_output_message("echo hi", 0, "hi", &theme),
            ),
            section(
                "user-memory-input",
                render_user_memory_input_message("prompt", "value", &theme),
            ),
            section("user-plan", render_user_plan_message("run tests", &theme)),
            section(
                "user-prompt",
                render_user_prompt_message("Continue", &theme),
            ),
            section(
                "user-resource-update",
                render_user_resource_update_message("memory", "+1GB", &theme),
            ),
            section(
                "user-teammate",
                render_user_teammate_message("charlie", "online", &theme),
            ),
            section("user-text", render_user_text_message("Hello world", &theme)),
        ]
        .join("\n\n");

        insta::assert_snapshot!("message_component_helpers", rendered);
    }

    fn section(name: &str, body: String) -> String {
        format!("## {name}\n{body}")
    }
}
