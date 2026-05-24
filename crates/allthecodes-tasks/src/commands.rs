//! `/tasks` command parsing and dispatch decisions.

use crate::TaskError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TasksCommand {
    List,
    Show { id: String },
    Stop { id: String },
    Delete { id: String },
    Unknown { subcommand: String },
}

pub fn parse_tasks_command(args: &str) -> Result<TasksCommand, TaskError> {
    let mut parts = args.split_whitespace();
    let sub = parts.next().unwrap_or("").to_ascii_lowercase();
    match sub.as_str() {
        "" | "list" | "ls" => Ok(TasksCommand::List),
        "show" | "info" => parse_id(&mut parts, "task_id", "Usage: /tasks show <id>")
            .map(|id| TasksCommand::Show { id }),
        "stop" | "kill" | "cancel" => parse_id(&mut parts, "task_id", "Usage: /tasks stop <id>")
            .map(|id| TasksCommand::Stop { id }),
        "delete" | "rm" => parse_id(&mut parts, "task_id", "Usage: /tasks delete <id>")
            .map(|id| TasksCommand::Delete { id }),
        other => Ok(TasksCommand::Unknown {
            subcommand: other.to_string(),
        }),
    }
}

fn parse_id<'a>(
    parts: &mut impl Iterator<Item = &'a str>,
    field: &'static str,
    usage: &'static str,
) -> Result<String, TaskError> {
    let id = parts.next().unwrap_or("").trim();
    if id.is_empty() {
        Err(TaskError::missing_field(field, usage))
    } else {
        Ok(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskErrorCode;

    #[test]
    fn parses_aliases() {
        assert_eq!(
            parse_tasks_command("rm 42").unwrap(),
            TasksCommand::Delete { id: "42".into() }
        );
        assert_eq!(parse_tasks_command("ls").unwrap(), TasksCommand::List);
    }

    #[test]
    fn missing_id_is_structured() {
        let error = parse_tasks_command("show").unwrap_err();
        assert_eq!(error.code, TaskErrorCode::MissingField);
        assert_eq!(error.field, Some("task_id"));
    }
}
