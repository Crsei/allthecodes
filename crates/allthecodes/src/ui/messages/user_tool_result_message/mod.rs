//! Rust-side renderers for user tool result messages.

#![allow(clippy::module_inception)]

pub mod rejected_plan_message;
pub mod rejected_tool_use_message;
pub mod user_tool_canceled_message;
pub mod user_tool_error_message;
pub mod user_tool_reject_message;
pub mod user_tool_result_message;
pub mod user_tool_success_message;
pub mod utils;

#[cfg(test)]
mod tests {
    use super::rejected_plan_message::render_rejected_plan_message;
    use super::rejected_tool_use_message::render_rejected_tool_use_message;
    use super::user_tool_canceled_message::render_user_tool_canceled_message;
    use super::user_tool_error_message::render_user_tool_error_message;
    use super::user_tool_result_message::render_user_tool_result_message;
    use super::user_tool_success_message::render_user_tool_success_message;
    use super::utils::{
        line_to_text, ToolResultBlock, UserToolResultLookups, CANCEL_MESSAGE,
        CLASSIFIER_DENIAL_PREFIX, INTERRUPT_MESSAGE_FOR_TOOL_USE, PLAN_REJECTION_PREFIX,
        REJECT_MESSAGE_WITH_REASON_PREFIX,
    };
    use crate::ui::theme::Theme;

    #[test]
    fn snapshot_user_tool_result_messages() {
        let theme = Theme::default();
        let lookup = UserToolResultLookups::new();
        let tools = Vec::new();
        let canceled = ToolResultBlock::new("tool-1", true, CANCEL_MESSAGE);
        let success =
            ToolResultBlock::new("tool-2", false, "").with_tool_use_result("{\"ok\":true}");
        let denied = format!("{CLASSIFIER_DENIAL_PREFIX}policy blocked command");
        let plan = format!("{PLAN_REJECTION_PREFIX}1. edit files\n2. run tests");

        let rendered = [
            section("canceled", render_user_tool_canceled_message(&theme)),
            section("rejected-tool", render_rejected_tool_use_message(&theme)),
            section(
                "rejected-plan",
                render_rejected_plan_message("check first", &theme),
            ),
            section(
                "success",
                render_user_tool_success_message(None, None, Some("done"), &theme, false, false),
            ),
            section(
                "error",
                render_user_tool_error_message(&denied, None, None, &theme, false, false),
            ),
            section(
                "plan-error",
                render_user_tool_error_message(&plan, None, None, &theme, false, false),
            ),
            section(
                "dispatch-cancel",
                render_user_tool_result_message(
                    &canceled, &lookup, &tools, &theme, false, 80, false,
                ),
            ),
            section(
                "dispatch-success",
                render_user_tool_result_message(
                    &success, &lookup, &tools, &theme, false, 80, false,
                ),
            ),
        ]
        .join("\n\n");

        insta::assert_snapshot!("user_tool_result_messages", rendered);
    }

    #[test]
    fn user_tool_result_messages_keep_rejection_reason_and_unknown_errors() {
        let theme = Theme::default();
        let lookup = UserToolResultLookups::new();
        let tools = Vec::new();

        let rejected_with_reason = ToolResultBlock::new(
            "tool-reject",
            true,
            format!("{REJECT_MESSAGE_WITH_REASON_PREFIX}please inspect first"),
        );
        let rejected_text = section(
            "reject-reason",
            render_user_tool_result_message(
                &rejected_with_reason,
                &lookup,
                &tools,
                &theme,
                false,
                80,
                false,
            ),
        );
        assert!(rejected_text.contains("reason included"));
        assert!(rejected_text.contains("please inspect first"));

        let unknown_error = ToolResultBlock::new("tool-error", true, "ENOENT: missing file");
        let error_text = section(
            "unknown-error",
            render_user_tool_result_message(
                &unknown_error,
                &lookup,
                &tools,
                &theme,
                false,
                80,
                false,
            ),
        );
        assert!(error_text.contains("Tool failed with error:"));
        assert!(error_text.contains("ENOENT: missing file"));

        let interrupted =
            ToolResultBlock::new("tool-interrupt", true, INTERRUPT_MESSAGE_FOR_TOOL_USE);
        assert_eq!(
            section(
                "interrupt",
                render_user_tool_result_message(
                    &interrupted,
                    &lookup,
                    &tools,
                    &theme,
                    false,
                    80,
                    false,
                ),
            ),
            "## interrupt\nInterrupted by user"
        );
    }

    fn section(name: &str, lines: Vec<ratatui::text::Line<'_>>) -> String {
        let body = lines
            .iter()
            .map(line_to_text)
            .collect::<Vec<_>>()
            .join("\n");
        format!("## {name}\n{body}")
    }
}
