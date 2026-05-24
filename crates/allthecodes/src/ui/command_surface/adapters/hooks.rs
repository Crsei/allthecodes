use std::collections::HashMap;

use allthecodes_types::hooks::{HookEvent, HOOK_EVENTS};
use serde_json::Value;

#[cfg(test)]
use crate::ui::hooks::hooks_config_menu::HookConfigSummary;

pub(crate) type HooksByEventAndMatcher =
    HashMap<HookEvent, HashMap<String, Vec<IndividualHookConfig>>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MatcherMetadata {
    pub(crate) field_to_match: String,
    pub(crate) values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookEventMetadata {
    pub(crate) summary: String,
    pub(crate) description: String,
    pub(crate) matcher_metadata: Option<MatcherMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HookSource {
    EffectiveSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndividualHookConfig {
    pub(crate) event: HookEvent,
    pub(crate) config: Value,
    pub(crate) matcher: String,
    pub(crate) source: HookSource,
    pub(crate) plugin_name: Option<String>,
}

pub(crate) fn hook_event_metadata(event: HookEvent, tool_names: &[String]) -> HookEventMetadata {
    match event {
        HookEvent::PreToolUse => HookEventMetadata {
            summary: "Before tool execution".into(),
            description: "Input to command is JSON of tool call arguments.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to model and block tool call\nOther exit codes - show stderr to user only but continue with tool call".into(),
            matcher_metadata: Some(tool_matcher(tool_names)),
        },
        HookEvent::PostToolUse => HookEventMetadata {
            summary: "After tool execution".into(),
            description: "Input to command is JSON with fields \"inputs\" (tool call arguments) and \"response\" (tool call response).\nExit code 0 - stdout shown in transcript mode (ctrl+o)\nExit code 2 - show stderr to model immediately\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(tool_matcher(tool_names)),
        },
        HookEvent::PostToolUseFailure => HookEventMetadata {
            summary: "After tool execution fails".into(),
            description: "Input to command is JSON with tool_name, tool_input, tool_use_id, error, error_type, is_interrupt, and is_timeout.\nExit code 0 - stdout shown in transcript mode (ctrl+o)\nExit code 2 - show stderr to model immediately\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(tool_matcher(tool_names)),
        },
        HookEvent::PermissionDenied => HookEventMetadata {
            summary: "After auto mode classifier denies a tool call".into(),
            description: "Input to command is JSON with tool_name, tool_input, tool_use_id, and reason.\nReturn hookSpecificOutput.retry=true to tell the model it may retry.\nExit code 0 - stdout shown in transcript mode (ctrl+o)\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(tool_matcher(tool_names)),
        },
        HookEvent::Notification => HookEventMetadata {
            summary: "When notifications are sent".into(),
            description: "Input to command is JSON with notification message and type.\nExit code 0 - stdout/stderr not shown\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "notification_type".into(),
                values: vec![
                    "permission_prompt".into(),
                    "idle_prompt".into(),
                    "auth_success".into(),
                    "elicitation_dialog".into(),
                    "elicitation_complete".into(),
                    "elicitation_response".into(),
                ],
            }),
        },
        HookEvent::UserPromptSubmit => HookEventMetadata {
            summary: "When the user submits a prompt".into(),
            description: "Input to command is JSON with original user prompt text.\nExit code 0 - stdout shown to Claude\nExit code 2 - block processing, erase original prompt, and show stderr to user only\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::SessionStart => HookEventMetadata {
            summary: "When a new session is started".into(),
            description: "Input to command is JSON with session start source.\nExit code 0 - stdout shown to Claude\nBlocking errors are ignored\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "source".into(),
                values: vec!["startup".into(), "resume".into(), "clear".into(), "compact".into()],
            }),
        },
        HookEvent::Stop => HookEventMetadata {
            summary: "Right before Claude concludes its response".into(),
            description: "Exit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to model and continue conversation\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::StopFailure => HookEventMetadata {
            summary: "When the turn ends due to an API error".into(),
            description: "Fires instead of Stop when an API error (rate limit, auth failure, etc.) ended the turn. Fire-and-forget; hook output and exit codes are ignored.".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "error".into(),
                values: vec![
                    "rate_limit".into(),
                    "authentication_failed".into(),
                    "billing_error".into(),
                    "invalid_request".into(),
                    "server_error".into(),
                    "max_output_tokens".into(),
                    "unknown".into(),
                ],
            }),
        },
        HookEvent::SubagentStart => HookEventMetadata {
            summary: "When a subagent (Agent tool call) is started".into(),
            description: "Input to command is JSON with agent_id and agent_type.\nExit code 0 - stdout shown to subagent\nBlocking errors are ignored\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "agent_type".into(),
                values: Vec::new(),
            }),
        },
        HookEvent::SubagentStop => HookEventMetadata {
            summary: "Right before a subagent (Agent tool call) concludes its response".into(),
            description: "Input to command is JSON with agent_id, agent_type, and agent_transcript_path.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to subagent and continue having it run\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "agent_type".into(),
                values: Vec::new(),
            }),
        },
        HookEvent::PreCompact => HookEventMetadata {
            summary: "Before conversation compaction".into(),
            description: "Input to command is JSON with compaction details.\nExit code 0 - stdout appended as custom compact instructions\nExit code 2 - block compaction\nOther exit codes - show stderr to user only but continue with compaction".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "trigger".into(),
                values: vec!["manual".into(), "auto".into()],
            }),
        },
        HookEvent::PostCompact => HookEventMetadata {
            summary: "After conversation compaction".into(),
            description: "Input to command is JSON with compaction details and the summary.\nExit code 0 - stdout shown to user\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "trigger".into(),
                values: vec!["manual".into(), "auto".into()],
            }),
        },
        HookEvent::SessionEnd => HookEventMetadata {
            summary: "When a session is ending".into(),
            description: "Input to command is JSON with session end reason.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "reason".into(),
                values: vec![
                    "clear".into(),
                    "logout".into(),
                    "prompt_input_exit".into(),
                    "other".into(),
                ],
            }),
        },
        HookEvent::PermissionRequest => HookEventMetadata {
            summary: "When a permission dialog is displayed".into(),
            description: "Input to command is JSON with tool_name, tool_input, and tool_use_id.\nOutput JSON with hookSpecificOutput containing decision to allow or deny.\nExit code 0 - use hook decision if provided\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(tool_matcher(tool_names)),
        },
        HookEvent::Setup => HookEventMetadata {
            summary: "Repo setup hooks for init and maintenance".into(),
            description: "Input to command is JSON with trigger (init or maintenance).\nExit code 0 - stdout shown to Claude\nBlocking errors are ignored\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "trigger".into(),
                values: vec!["init".into(), "maintenance".into()],
            }),
        },
        HookEvent::TeammateIdle => HookEventMetadata {
            summary: "When a teammate is about to go idle".into(),
            description: "Input to command is JSON with teammate_name and team_name.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to teammate and prevent idle\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::TaskCreated => HookEventMetadata {
            summary: "When a task is being created".into(),
            description: "Input to command is JSON with task_id, task_subject, task_description, teammate_name, and team_name.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to model and prevent task creation\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::TaskCompleted => HookEventMetadata {
            summary: "When a task is being marked as completed".into(),
            description: "Input to command is JSON with task_id, task_subject, task_description, teammate_name, and team_name.\nExit code 0 - stdout/stderr not shown\nExit code 2 - show stderr to model and prevent task completion\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::Elicitation => HookEventMetadata {
            summary: "When an MCP server requests user input".into(),
            description: "Input to command is JSON with mcp_server_name, message, and requested_schema.\nOutput JSON with hookSpecificOutput containing action and optional content.\nExit code 0 - use hook response if provided\nExit code 2 - deny the elicitation\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "mcp_server_name".into(),
                values: Vec::new(),
            }),
        },
        HookEvent::ElicitationResult => HookEventMetadata {
            summary: "After a user responds to an MCP elicitation".into(),
            description: "Input to command is JSON with mcp_server_name, action, content, mode, and elicitation_id.\nOutput JSON with hookSpecificOutput containing optional action and content to override the response.\nExit code 0 - use hook response if provided\nExit code 2 - block the response\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "mcp_server_name".into(),
                values: Vec::new(),
            }),
        },
        HookEvent::ConfigChange => HookEventMetadata {
            summary: "When configuration files change during a session".into(),
            description: "Input to command is JSON with source and file_path.\nExit code 0 - allow the change\nExit code 2 - block the change from being applied to the session\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "source".into(),
                values: vec![
                    "user_settings".into(),
                    "project_settings".into(),
                    "local_settings".into(),
                    "policy_settings".into(),
                    "skills".into(),
                ],
            }),
        },
        HookEvent::InstructionsLoaded => HookEventMetadata {
            summary: "When an instruction file is loaded".into(),
            description: "Input to command is JSON with file_path, memory_type, load_reason, globs, trigger_file_path, and parent_file_path.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only\nThis hook is observability-only and does not support blocking.".into(),
            matcher_metadata: Some(MatcherMetadata {
                field_to_match: "load_reason".into(),
                values: vec![
                    "session_start".into(),
                    "nested_traversal".into(),
                    "path_glob_match".into(),
                    "include".into(),
                    "compact".into(),
                ],
            }),
        },
        HookEvent::WorktreeCreate => HookEventMetadata {
            summary: "Create an isolated worktree".into(),
            description: "Input to command is JSON with name (suggested worktree slug).\nStdout should contain the absolute path to the created worktree directory.\nExit code 0 - worktree created successfully\nOther exit codes - worktree creation failed".into(),
            matcher_metadata: None,
        },
        HookEvent::WorktreeRemove => HookEventMetadata {
            summary: "Remove a previously created worktree".into(),
            description: "Input to command is JSON with worktree_path (absolute path to worktree).\nExit code 0 - worktree removed successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::CwdChanged => HookEventMetadata {
            summary: "After the working directory changes".into(),
            description: "Input to command is JSON with old_cwd and new_cwd.\nCLAUDE_ENV_FILE is set; write bash exports there to apply env to subsequent BashTool commands.\nHook output can include hookSpecificOutput.watchPaths to register with the FileChanged watcher.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
        HookEvent::FileChanged => HookEventMetadata {
            summary: "When a watched file changes".into(),
            description: "Input to command is JSON with file_path and event (change, add, unlink).\nCLAUDE_ENV_FILE is set; write bash exports there to apply env to subsequent BashTool commands.\nThe matcher field specifies filenames to watch in the current directory.\nHook output can include hookSpecificOutput.watchPaths to dynamically update the watch list.\nExit code 0 - command completes successfully\nOther exit codes - show stderr to user only".into(),
            matcher_metadata: None,
        },
    }
}

#[cfg(test)]
pub(crate) fn hook_summary(event: HookEvent, value: Option<&Value>) -> HookConfigSummary {
    let Some(configs) = value.and_then(Value::as_array) else {
        return HookConfigSummary {
            event,
            matcher_count: 0,
            hook_count: 0,
        };
    };

    let hook_count = configs
        .iter()
        .filter_map(|config| config.get("hooks"))
        .filter_map(Value::as_array)
        .map(Vec::len)
        .sum();
    HookConfigSummary {
        event,
        matcher_count: configs.len(),
        hook_count,
    }
}

pub(crate) fn group_hooks_by_event_and_matcher(
    hooks: &HashMap<String, Value>,
) -> HooksByEventAndMatcher {
    let mut grouped = HOOK_EVENTS
        .iter()
        .copied()
        .map(|event| (event, HashMap::new()))
        .collect::<HooksByEventAndMatcher>();

    for event in HOOK_EVENTS.iter().copied() {
        let metadata = hook_event_metadata(event, &[]);
        let Some(configs) = hooks.get(&event.to_string()).and_then(Value::as_array) else {
            continue;
        };

        for matcher_config in configs {
            let configured_matcher = matcher_config
                .get("matcher")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let matcher_key = if metadata.matcher_metadata.is_some() {
                configured_matcher
            } else {
                String::new()
            };

            let Some(hook_values) = matcher_config.get("hooks").and_then(Value::as_array) else {
                continue;
            };
            let event_group = grouped.entry(event).or_default();
            let matcher_group = event_group.entry(matcher_key.clone()).or_default();
            matcher_group.extend(
                hook_values
                    .iter()
                    .cloned()
                    .map(|config| IndividualHookConfig {
                        event,
                        config,
                        matcher: matcher_key.clone(),
                        source: HookSource::EffectiveSettings,
                        plugin_name: None,
                    }),
            );
        }
    }

    grouped
}

pub(crate) fn get_sorted_matchers_for_event(
    hooks_by_event_and_matcher: &HooksByEventAndMatcher,
    event: HookEvent,
) -> Vec<String> {
    let Some(matchers) = hooks_by_event_and_matcher.get(&event) else {
        return Vec::new();
    };

    let mut result = matchers.keys().cloned().collect::<Vec<_>>();
    result.sort_by(|a, b| {
        let a_priority = highest_source_priority(matchers.get(a).map(Vec::as_slice).unwrap_or(&[]));
        let b_priority = highest_source_priority(matchers.get(b).map(Vec::as_slice).unwrap_or(&[]));
        a_priority.cmp(&b_priority).then_with(|| a.cmp(b))
    });
    result
}

pub(crate) fn get_hooks_for_matcher(
    hooks_by_event_and_matcher: &HooksByEventAndMatcher,
    event: HookEvent,
    matcher: &str,
) -> Vec<IndividualHookConfig> {
    hooks_by_event_and_matcher
        .get(&event)
        .and_then(|event_group| event_group.get(matcher))
        .cloned()
        .unwrap_or_default()
}

pub(crate) fn hooks_by_event_count(
    hooks_by_event_and_matcher: &HooksByEventAndMatcher,
) -> HashMap<HookEvent, usize> {
    hooks_by_event_and_matcher
        .iter()
        .map(|(event, matchers)| {
            (
                *event,
                matchers.values().map(|hooks| hooks.len()).sum::<usize>(),
            )
        })
        .collect()
}

pub(crate) fn hook_type(config: &Value) -> String {
    config
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| {
            if config.get("command").is_some() {
                Some("command")
            } else if config.get("url").is_some() {
                Some("http")
            } else if config.get("prompt").is_some() {
                Some("prompt")
            } else {
                None
            }
        })
        .unwrap_or("unknown")
        .to_string()
}

pub(crate) fn hook_display_text(config: &Value) -> String {
    if let Some(status) = string_field(config, "statusMessage") {
        return status;
    }

    hook_primary_content(config).unwrap_or_else(|| compact_json(config))
}

pub(crate) fn hook_primary_field(config: &Value) -> (&'static str, String) {
    let hook_type = hook_type(config);
    match hook_type.as_str() {
        "command" => (
            "Command",
            string_field(config, "command").unwrap_or_else(|| compact_json(config)),
        ),
        "prompt" | "agent" => (
            "Prompt",
            string_field(config, "prompt").unwrap_or_else(|| compact_json(config)),
        ),
        "http" => (
            "URL",
            string_field(config, "url").unwrap_or_else(|| compact_json(config)),
        ),
        _ => ("Config", compact_json(config)),
    }
}

pub(crate) fn hook_source_description(source: HookSource) -> &'static str {
    match source {
        HookSource::EffectiveSettings => "Effective settings (merged runtime hooks)",
    }
}

pub(crate) fn hook_source_header(source: HookSource) -> &'static str {
    match source {
        HookSource::EffectiveSettings => "Effective Settings",
    }
}

pub(crate) fn hook_source_inline(source: HookSource) -> &'static str {
    match source {
        HookSource::EffectiveSettings => "Effective",
    }
}

pub(crate) fn matcher_display_label(matcher: &str) -> String {
    match matcher {
        "" => "(all)".to_string(),
        "*" => "All tools (*)".to_string(),
        other => other.to_string(),
    }
}

fn tool_matcher(tool_names: &[String]) -> MatcherMetadata {
    MatcherMetadata {
        field_to_match: "tool_name".into(),
        values: tool_names.to_vec(),
    }
}

fn highest_source_priority(hooks: &[IndividualHookConfig]) -> u32 {
    hooks
        .iter()
        .map(|hook| source_priority(hook.source))
        .min()
        .unwrap_or(u32::MAX)
}

fn source_priority(source: HookSource) -> u32 {
    match source {
        HookSource::EffectiveSettings => 100,
    }
}

fn hook_primary_content(config: &Value) -> Option<String> {
    match hook_type(config).as_str() {
        "command" => string_field(config, "command"),
        "prompt" | "agent" => string_field(config, "prompt"),
        "http" => string_field(config, "url"),
        _ => None,
    }
}

fn string_field(config: &Value, field: &str) -> Option<String> {
    config
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "<invalid hook config>".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_runtime_hooks_by_canonical_event_and_matcher() {
        let hooks = HashMap::from([
            (
                "PreToolUse".to_string(),
                json!([{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "cargo test" }] }]),
            ),
            (
                "Stop".to_string(),
                json!([{ "matcher": "ignored", "hooks": [{ "type": "prompt", "prompt": "summarize" }] }]),
            ),
        ]);

        let grouped = group_hooks_by_event_and_matcher(&hooks);

        assert_eq!(
            grouped[&HookEvent::PreToolUse]["Bash"][0].config["command"],
            "cargo test"
        );
        assert_eq!(
            grouped[&HookEvent::Stop][""][0].config["prompt"],
            "summarize"
        );
        assert!(HOOK_EVENTS.iter().all(|event| grouped.contains_key(event)));
    }

    #[test]
    fn hook_display_text_supports_all_read_only_types() {
        assert_eq!(
            hook_display_text(&json!({ "type": "command", "command": "cargo test" })),
            "cargo test"
        );
        assert_eq!(
            hook_display_text(&json!({ "type": "prompt", "prompt": "review this" })),
            "review this"
        );
        assert_eq!(
            hook_display_text(&json!({ "type": "agent", "prompt": "inspect repo" })),
            "inspect repo"
        );
        assert_eq!(
            hook_display_text(&json!({ "type": "http", "url": "https://example.test/hook" })),
            "https://example.test/hook"
        );
        assert_eq!(
            hook_display_text(&json!({
                "type": "command",
                "command": "cargo test",
                "statusMessage": "Run tests"
            })),
            "Run tests"
        );
    }
}
