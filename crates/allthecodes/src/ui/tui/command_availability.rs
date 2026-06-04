use allthecodes_commands::CommandMetadata;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TaskCommandAvailability {
    NotCommand,
    UnknownCommand,
    Allowed,
    Disabled { command: String },
}

pub(super) fn slash_command_availability_during_task(input: &str) -> TaskCommandAvailability {
    let Some(command_name) = leading_slash_command_name(input) else {
        return TaskCommandAvailability::NotCommand;
    };

    let metadata = allthecodes_commands::get_dynamic_metadata();
    let Some(command) = resolve_command(&command_name, &metadata) else {
        return TaskCommandAvailability::UnknownCommand;
    };

    if command.name == "goal" && is_goal_status_command(input) {
        TaskCommandAvailability::Allowed
    } else if is_available_during_task(&command.name) {
        TaskCommandAvailability::Allowed
    } else {
        TaskCommandAvailability::Disabled {
            command: command.name.clone(),
        }
    }
}

fn leading_slash_command_name(input: &str) -> Option<&str> {
    let trimmed = input.trim_start();
    let without_slash = trimmed.strip_prefix('/')?;
    without_slash
        .split_whitespace()
        .next()
        .filter(|name| !name.is_empty())
}

fn is_goal_status_command(input: &str) -> bool {
    let Some(command_name) = leading_slash_command_name(input) else {
        return false;
    };
    let trimmed = input.trim_start();
    let without_slash = trimmed.trim_start_matches('/');
    let args = without_slash
        .strip_prefix(command_name)
        .unwrap_or_default()
        .trim();
    let (verb, _) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    matches!(
        verb,
        "" | "status" | "show" | "help" | "-h" | "--help" | "pause" | "resume" | "clear"
    )
}

fn resolve_command<'a>(
    command_name: &str,
    metadata: &'a [CommandMetadata],
) -> Option<&'a CommandMetadata> {
    metadata.iter().find(|command| {
        command.name == command_name || command.aliases.iter().any(|alias| alias == command_name)
    })
}

fn is_available_during_task(command: &str) -> bool {
    matches!(
        command,
        "help"
            | "diff"
            | "debug"
            | "exit"
            | "version"
            | "cost"
            | "session"
            | "rename"
            | "files"
            | "context"
            | "copy"
            | "status"
            | "export"
            | "audit-export"
            | "session-export"
            | "extra-usage"
            | "rate-limit-options"
            | "mcp"
            | "ide"
            | "lsp"
            | "plugin"
            | "skills"
            | "chrome"
            | "assistant"
            | "daemon"
            | "notify"
            | "remote"
            | "channels"
            | "memory"
            | "hooks"
            | "agents"
            | "doctor"
            | "tasks"
            | "team"
            | "terminal-setup"
            | "voice"
            | "statusline"
            | "recap"
    )
}

#[cfg(test)]
mod tests {
    use super::{slash_command_availability_during_task, TaskCommandAvailability};

    fn disabled(input: &str) -> String {
        match slash_command_availability_during_task(input) {
            TaskCommandAvailability::Disabled { command } => command,
            other => panic!("expected disabled command, got {other:?}"),
        }
    }

    #[test]
    fn allows_commands_available_while_task_is_running() {
        assert_eq!(
            slash_command_availability_during_task("/status"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(
            slash_command_availability_during_task("/diff --stat"),
            TaskCommandAvailability::Allowed
        );
    }

    #[test]
    fn disables_commands_unavailable_while_task_is_running() {
        assert_eq!(disabled("/review these changes"), "review");
        assert_eq!(disabled("/clear"), "clear");
        assert_eq!(disabled("/model opus"), "model");
        assert_eq!(disabled("/goal ship the release"), "goal");
        assert_eq!(disabled("/goal complete shipped"), "goal");
        assert_eq!(disabled("/goal block waiting on input"), "goal");
    }

    #[test]
    fn allows_goal_status_while_task_is_running() {
        assert_eq!(
            slash_command_availability_during_task("/goal"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(
            slash_command_availability_during_task("/goal status"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(
            slash_command_availability_during_task("/goal pause waiting"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(
            slash_command_availability_during_task("/goal resume"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(
            slash_command_availability_during_task("/goal clear"),
            TaskCommandAvailability::Allowed
        );
    }

    #[test]
    fn resolves_aliases_before_checking_availability() {
        assert_eq!(
            slash_command_availability_during_task("/quit"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(
            slash_command_availability_during_task("/q"),
            TaskCommandAvailability::Allowed
        );
        assert_eq!(disabled("/settings"), "config");
    }

    #[test]
    fn unknown_and_non_leading_slash_input_are_not_intercepted() {
        assert_eq!(
            slash_command_availability_during_task("/definitely-unknown-command"),
            TaskCommandAvailability::UnknownCommand
        );
        assert_eq!(
            slash_command_availability_during_task("please run /review"),
            TaskCommandAvailability::NotCommand
        );
    }

    #[test]
    fn leading_space_slash_command_is_still_a_command() {
        assert_eq!(disabled("  /review these changes"), "review");
    }
}
