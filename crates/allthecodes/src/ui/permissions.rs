// BEGIN generated upstream permissions modules
// Rust-side permission modules mirrored from upstream React components.
pub mod ask_user_question_permission_request;
pub mod bash_permission_request;
pub mod bypass_permissions_mode_dialog;
pub mod computer_use_approval;
pub mod enter_plan_mode_permission_request;
pub mod exit_plan_mode_permission_request;
pub mod fallback_permission_request;
pub mod file_edit_permission_request;
pub mod file_permission_dialog;
pub mod file_write_permission_request;
pub mod filesystem_permission_request;
pub mod hooks;
pub mod monitor_permission_request;
pub mod notebook_edit_permission_request;
pub mod permission_decision_debug_info;
#[cfg(test)]
pub mod permission_dialog;
#[cfg(test)]
pub mod permission_explanation;
#[cfg(test)]
pub mod permission_prompt;
#[cfg(test)]
pub mod permission_request;
pub mod permission_request_router;
#[cfg(test)]
pub mod permission_request_title;
#[cfg(test)]
pub mod permission_rule_explanation;
pub mod power_shell_permission_request;
pub mod question_dialog;
pub mod review_artifact_permission_request;
pub mod rules;
pub mod sandbox_permission_request;
pub mod sed_edit_permission_request;
pub mod shell_permission_helpers;
pub mod skill_permission_request;
#[cfg(test)]
pub mod use_shell_permission_feedback;
pub mod utils;
pub mod web_fetch_permission_request;
pub mod worker_badge;
pub mod worker_pending_permission;
// END generated upstream permissions modules

mod dialog_overlay;

pub use bypass_permissions_mode_dialog::{
    BypassPermissionsModeChoice, BypassPermissionsModeDialog,
};
pub use dialog_overlay::{PermissionChoice, PermissionDecisionChoice, PermissionDialog};
pub use permission_request_router::PermissionDialogRequest;
pub use question_dialog::QuestionDialog;

#[cfg(test)]
mod tests {
    use super::ask_user_question_permission_request::ask_user_question_permission_request::render_ask_user_question_permission_request;
    use super::ask_user_question_permission_request::preview_box::render_preview_box;
    use super::ask_user_question_permission_request::preview_question_view::render_preview_question_view;
    use super::ask_user_question_permission_request::question_navigation_bar::render_question_navigation_bar;
    use super::ask_user_question_permission_request::question_view::render_question_view;
    use super::ask_user_question_permission_request::submit_questions_view::render_submit_questions_view;
    use super::ask_user_question_permission_request::use_multiple_choice_state::{
        render_multiple_choice_state, MultipleChoiceState,
    };
    use super::bash_permission_request::bash_permission_request::render_bash_permission_request;
    use super::bash_permission_request::bash_tool_use_options::render_bash_tool_use_options;
    use super::computer_use_approval::computer_use_approval::render_computer_use_approval;
    use super::enter_plan_mode_permission_request::enter_plan_mode_permission_request::render_enter_plan_mode_permission_request;
    use super::exit_plan_mode_permission_request::exit_plan_mode_permission_request::render_exit_plan_mode_permission_request;
    use super::fallback_permission_request::render_fallback_permission_request;
    use super::file_edit_permission_request::file_edit_permission_request::{
        render_file_edit_permission_request, render_file_edit_permission_request_with_diff,
    };
    use super::file_permission_dialog::file_permission_dialog::render_file_permission_dialog;
    use super::file_permission_dialog::ide_diff_config::{render_ide_diff_config, IdeDiffConfig};
    use super::file_permission_dialog::permission_options::file_permission_options;
    use super::file_permission_dialog::use_file_permission_dialog::FilePermissionDialogState;
    use super::file_permission_dialog::use_permission_handler::{
        describe_file_permission_decision, FilePermissionDecision,
    };
    use super::file_write_permission_request::file_write_permission_request::render_file_write_permission_request;
    use super::file_write_permission_request::file_write_tool_diff::render_file_write_tool_diff;
    use super::filesystem_permission_request::filesystem_permission_request::render_filesystem_permission_request;
    use super::hooks::{render_permission_hook_event, PermissionHookEvent};
    use super::monitor_permission_request::monitor_permission_request::render_monitor_permission_request;
    use super::notebook_edit_permission_request::notebook_edit_permission_request::render_notebook_edit_permission_request;
    use super::notebook_edit_permission_request::notebook_edit_tool_diff::render_notebook_edit_tool_diff;
    use super::permission_decision_debug_info::render_permission_decision_debug_info;
    use super::permission_dialog::render_permission_dialog_summary;
    use super::permission_explanation::render_permission_explanation;
    use super::permission_prompt::{render_permission_prompt, PermissionPromptState};
    use super::permission_request::{basic_permission_request, render_permission_request_surface};
    use super::permission_request_title::render_permission_request_title;
    use super::permission_rule_explanation::render_permission_rule_explanation;
    use super::power_shell_permission_request::power_shell_permission_request::render_power_shell_permission_request;
    use super::power_shell_permission_request::powershell_tool_use_options::render_powershell_tool_use_options;
    use super::review_artifact_permission_request::review_artifact_permission_request::render_review_artifact_permission_request;
    use super::rules::add_permission_rules::render_add_permission_rules;
    use super::rules::add_workspace_directory::render_add_workspace_directory;
    use super::rules::permission_rule_description::render_permission_rule_description;
    use super::rules::permission_rule_input::{
        render_permission_rule_input, validate_rule_pattern, PermissionRuleInputState,
    };
    use super::rules::permission_rule_list::render_permission_rule_list;
    use super::rules::recent_denials_tab::render_recent_denials_tab;
    use super::rules::remove_workspace_directory::render_remove_workspace_directory;
    use super::rules::workspace_tab::render_workspace_tab;
    use super::rules::{PermissionRule, RecentDenial, WorkspaceDirectory};
    use super::sandbox_permission_request::render_sandbox_permission_request;
    use super::sed_edit_permission_request::sed_edit_permission_request::render_sed_edit_permission_request;
    use super::shell_permission_helpers::{
        render_shell_permission_details, shell_permission_options, ShellKind,
    };
    use super::skill_permission_request::skill_permission_request::render_skill_permission_request;
    use super::use_shell_permission_feedback::{
        render_shell_permission_feedback, ShellPermissionFeedback,
    };
    use super::utils::{
        command_preview, default_permission_options, normalize_multiline, path_action_summary,
        render_bullets, render_key_values, render_permission_options, render_permission_request,
        shell_risk_hint, truncate_middle, PermissionDecision, PermissionRequestView,
        PermissionScope,
    };
    use super::web_fetch_permission_request::web_fetch_permission_request::render_web_fetch_permission_request;
    use super::worker_badge::render_worker_badge;
    use super::worker_pending_permission::render_worker_pending_permission;
    use crate::ui::diff::file_edit_diff::unified_hunk_lines_from_edit;

    #[test]
    fn snapshot_permission_component_helpers() {
        let edit_hunks = unified_hunk_lines_from_edit(
            "src/lib.rs",
            "fn main() {\n    old_call();\n}\n",
            "fn main() {\n    new_call();\n    extra_call();\n}\n",
        );
        let mut choices = MultipleChoiceState::new(vec![
            "Inspect logs".to_string(),
            "Ask operator".to_string(),
            "Abort".to_string(),
        ]);
        choices.select_next();
        choices.toggle_selected();

        let rules = vec![
            PermissionRule::new(
                "Bash(ls*)",
                PermissionDecision::Allow,
                PermissionScope::Project,
                ".allthecodes/settings.json",
            ),
            PermissionRule::new(
                "Write(src/**)",
                PermissionDecision::Deny,
                PermissionScope::Local,
                "policy",
            ),
        ];
        let directories = vec![
            WorkspaceDirectory {
                path: "F:/repo".to_string(),
                trusted: true,
            },
            WorkspaceDirectory {
                path: "F:/repo/tmp".to_string(),
                trusted: false,
            },
        ];
        let denials = vec![RecentDenial {
            tool_name: "bash".to_string(),
            pattern: "rm -rf".to_string(),
            reason: "destructive command".to_string(),
        }];
        let rule_input =
            PermissionRuleInputState::new("Bash(cargo test*)", PermissionDecision::Allow);
        let prompt =
            PermissionPromptState::new("Allow this action?", default_permission_options(), 2);
        let base_request = PermissionRequestView::new("Permission required", "bash", "cargo test")
            .with_detail("cwd: F:/repo")
            .with_risk("shell command")
            .for_worker("executor");
        let mut file_dialog = FilePermissionDialogState::new("src/lib.rs");
        file_dialog.select_next(3);

        let rendered = [
            section("utils-request", render_permission_request(&base_request)),
            section(
                "utils-options",
                render_permission_options(&default_permission_options(), 1).join("\n"),
            ),
            section(
                "utils-key-values",
                render_key_values("Rows", &[("tool", "bash"), ("decision", "allow")]),
            ),
            section(
                "utils-bullets",
                render_bullets("Notes", &["first", "second"]),
            ),
            section("utils-truncate", truncate_middle("abcdefghijklmnop", 10)),
            section("utils-normalize", normalize_multiline("a\n\nb").join("|")),
            section(
                "utils-command-preview",
                command_preview("cargo test\ncargo fmt"),
            ),
            section("utils-risk", shell_risk_hint("rm -rf target").to_string()),
            section(
                "utils-path-action",
                path_action_summary("edit", "src/main.rs"),
            ),
            section(
                "permission-request",
                render_permission_request_surface(&basic_permission_request("read", "src/lib.rs")),
            ),
            section("permission-prompt", render_permission_prompt(&prompt)),
            section(
                "permission-title",
                render_permission_request_title("bash", "cargo test", true),
            ),
            section(
                "permission-explanation",
                render_permission_explanation("writes project files", &["updates lockfile"]),
            ),
            section(
                "permission-rule-explanation",
                render_permission_rule_explanation(
                    "Bash(cargo*)",
                    PermissionDecision::AlwaysAllow,
                    PermissionScope::Project,
                ),
            ),
            section(
                "permission-debug",
                render_permission_decision_debug_info(
                    "bash",
                    "Bash(cargo*)",
                    "project",
                    Some("allow Bash(cargo*)"),
                ),
            ),
            section(
                "permission-dialog",
                render_permission_dialog_summary(&base_request, 80),
            ),
            section(
                "fallback",
                render_fallback_permission_request(
                    "custom_tool",
                    "run custom action",
                    &["opaque input"],
                ),
            ),
            section(
                "sandbox",
                render_sandbox_permission_request("read-only", "write target", &["target denied"]),
            ),
            section(
                "hooks",
                render_permission_hook_event(&PermissionHookEvent {
                    hook_name: "PreToolUse".to_string(),
                    matcher: "Bash(*)".to_string(),
                    decision: "allow".to_string(),
                    notes: vec!["matched project hook".to_string()],
                }),
            ),
            section(
                "shell-details",
                render_shell_permission_details(ShellKind::Bash, "cargo test"),
            ),
            section(
                "shell-options",
                render_permission_options(&shell_permission_options("curl https://example.com"), 0)
                    .join("\n"),
            ),
            section(
                "shell-feedback",
                render_shell_permission_feedback(&ShellPermissionFeedback {
                    command: "cargo test".to_string(),
                    accepted: true,
                    rule_saved: false,
                    message: "running now".to_string(),
                }),
            ),
            section("worker-badge", render_worker_badge("executor", 2)),
            section(
                "worker-pending",
                render_worker_pending_permission("executor", "write", "src/lib.rs", 1),
            ),
            section("ask-choice-state", render_multiple_choice_state(&choices)),
            section(
                "ask-preview-box",
                render_preview_box("Question", "Pick one"),
            ),
            section(
                "ask-preview-question",
                render_preview_question_view("Which path?", &["src", "tests"]),
            ),
            section(
                "ask-navigation",
                render_question_navigation_bar(2, 3, true, true),
            ),
            section(
                "ask-question-view",
                render_question_view("Choose", &choices),
            ),
            section("ask-submit", render_submit_questions_view(2, 3)),
            section(
                "ask-request",
                render_ask_user_question_permission_request("Need input", &choices, 2, 3),
            ),
            section(
                "bash-request",
                render_bash_permission_request("cargo test", 0),
            ),
            section(
                "bash-options",
                render_bash_tool_use_options("cargo test", 2),
            ),
            section(
                "powershell-request",
                render_power_shell_permission_request("Get-ChildItem", 1),
            ),
            section(
                "powershell-options",
                render_powershell_tool_use_options("Get-ChildItem", 0),
            ),
            section(
                "ide-diff",
                render_ide_diff_config(&IdeDiffConfig {
                    enabled: true,
                    editor: "code".to_string(),
                    supports_inline_diff: true,
                }),
            ),
            section(
                "file-options",
                render_permission_options(&file_permission_options("src/lib.rs", true), 2)
                    .join("\n"),
            ),
            section(
                "file-handler",
                describe_file_permission_decision(&FilePermissionDecision {
                    decision: PermissionDecision::Allow,
                    path: "src/lib.rs".to_string(),
                    save_rule: true,
                }),
            ),
            section(
                "file-dialog",
                render_file_permission_dialog(&file_dialog, "edit", "+3 -1"),
            ),
            section(
                "file-write-diff",
                render_file_write_tool_diff("src/lib.rs", 3, 1),
            ),
            section(
                "file-write-request",
                render_file_write_permission_request("src/lib.rs", 3, 1, 1),
            ),
            section(
                "file-edit-request",
                render_file_edit_permission_request("src/lib.rs", "replace range", 0),
            ),
            section(
                "file-edit-request-diff",
                render_file_edit_permission_request_with_diff(
                    "src/lib.rs",
                    "replace range",
                    &edit_hunks,
                    0,
                    80,
                ),
            ),
            section(
                "filesystem-request",
                render_filesystem_permission_request("read", "src", 2),
            ),
            section(
                "computer-use",
                render_computer_use_approval("click", "browser window", true),
            ),
            section(
                "enter-plan",
                render_enter_plan_mode_permission_request("Need design first"),
            ),
            section(
                "exit-plan",
                render_exit_plan_mode_permission_request("implement helpers", &["cargo test"]),
            ),
            section(
                "monitor",
                render_monitor_permission_request("cargo test", 60, 1),
            ),
            section(
                "review-artifact",
                render_review_artifact_permission_request("docs/report.md", "reviewer"),
            ),
            section(
                "sed-edit",
                render_sed_edit_permission_request("src/lib.rs", "foo", "bar", 1),
            ),
            section(
                "notebook-diff",
                render_notebook_edit_tool_diff("analysis.ipynb", 2, "python"),
            ),
            section(
                "notebook-request",
                render_notebook_edit_permission_request("analysis.ipynb", 2, "python", 0),
            ),
            section(
                "skill-request",
                render_skill_permission_request("lint-fix", "run skill", 1),
            ),
            section(
                "web-fetch",
                render_web_fetch_permission_request("https://example.com", "GET", 2),
            ),
            section(
                "rule-description",
                render_permission_rule_description(&rules[0]),
            ),
            section("rule-input", render_permission_rule_input(&rule_input)),
            section(
                "rule-input-validation",
                validate_rule_pattern("bad\npattern").unwrap_err(),
            ),
            section("rule-list", render_permission_rule_list(&rules, 1)),
            section(
                "add-rules",
                render_add_permission_rules(std::slice::from_ref(&rule_input)),
            ),
            section(
                "add-workspace",
                render_add_workspace_directory("F:/repo", true),
            ),
            section(
                "remove-workspace",
                render_remove_workspace_directory("F:/repo/tmp", 2),
            ),
            section("workspace-tab", render_workspace_tab(&directories, 0)),
            section("recent-denials", render_recent_denials_tab(&denials)),
        ]
        .join("\n\n");

        insta::assert_snapshot!("permission_component_helpers", rendered);
    }

    fn section(name: &str, body: String) -> String {
        format!("## {name}\n{body}")
    }
}
