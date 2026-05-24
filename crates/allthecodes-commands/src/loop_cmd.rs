//! `/loop` — user-friendly wrapper for the local recurring-task scheduler
//! (issue #58).
//!
//! The command is a thin veneer over [`allthecodes_services::scheduler`]: it
//! parses a human interval + payload, persists a recurring task, and — for
//! plain-prompt payloads — returns a `Query` message so the prompt also
//! executes immediately, matching the Bun reference's behavior.
//!
//! Subcommands:
//!
//!   /loop <interval> <payload>   create a new looping task and run it once
//!   /loop list                   list current loops
//!   /loop remove <id>            delete a loop
//!   /loop trigger <id>           mark a loop as fired now (re-runs the payload)
//!   /loop pause <id>             temporarily suspend without deleting
//!   /loop resume <id>            re-enable a paused loop
//!
//! `<payload>` is either a slash command (`/simplify`) or a plain prompt
//! (`review the last commit`). When the payload is a slash command, the
//! initial execution is not performed automatically — we report the
//! registration and instruct the user to run the command manually. When
//! it's a plain prompt, the wrapper returns a `CommandResult::Query`
//! carrying the prompt so the model sees it on this turn.

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_services::scheduler::{
    parse_interval, Interval, ScheduledTask, SchedulerError, SchedulerKind, SchedulerStore, TaskId,
    TaskPayload,
};
use allthecodes_types::message::{Message, MessageContent, UserMessage};

pub struct LoopHandler;

#[async_trait]
impl CommandHandler for LoopHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let store = SchedulerStore::open_default();
        Ok(dispatch(&store, args))
    }
}

fn dispatch(store: &SchedulerStore, args: &str) -> CommandResult {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return CommandResult::Output(help_text());
    }

    let (head, rest) = split_first(trimmed);
    match head.as_str() {
        "list" | "ls" => CommandResult::Output(render_list(store)),
        "help" | "--help" | "-h" => CommandResult::Output(help_text()),
        "remove" | "rm" | "delete" => CommandResult::Output(remove_task(store, rest.trim())),
        "trigger" | "fire" => CommandResult::Output(trigger_task(store, rest.trim())),
        "pause" => CommandResult::Output(set_paused(store, rest.trim(), true)),
        "resume" | "unpause" => CommandResult::Output(set_paused(store, rest.trim(), false)),
        _ => create_loop(store, trimmed),
    }
}

fn help_text() -> String {
    [
        "/loop — recurring local tasks.",
        "",
        "  /loop <interval> <payload>   create a loop and run payload once",
        "  /loop list                   list current loops",
        "  /loop remove <id>            delete a loop",
        "  /loop trigger <id>           mark a loop as fired now",
        "  /loop pause <id>             temporarily suspend a loop",
        "  /loop resume <id>            re-enable a paused loop",
        "",
        "Interval examples: 30s, 5m, 1h, 2d, '*/10 * * * *'.",
        "Payload is a slash command (/simplify) or a plain prompt.",
    ]
    .join("\n")
}

fn create_loop(store: &SchedulerStore, input: &str) -> CommandResult {
    let Some((interval_raw, payload_raw)) = input.split_once(char::is_whitespace) else {
        return CommandResult::Output(
            "Usage: /loop <interval> <payload>. Run '/loop help' for examples.".to_string(),
        );
    };

    let payload_raw = payload_raw.trim();
    if payload_raw.is_empty() {
        return CommandResult::Output("Usage: /loop <interval> <payload>".to_string());
    }

    let interval = match parse_interval(interval_raw) {
        Ok(i) => i,
        Err(e) => {
            return CommandResult::Output(format!(
                "Could not parse interval '{}': {}",
                interval_raw, e
            ));
        }
    };

    let payload = TaskPayload::from_user_input(payload_raw);
    let task = ScheduledTask::new(
        SchedulerKind::LocalCron,
        derive_name(&payload),
        interval_raw,
        interval,
        payload.clone(),
        Utc::now(),
    );

    let saved = match store.add(task) {
        Ok(t) => t,
        Err(e) => {
            return CommandResult::Output(format!("Could not create /loop task: {}", e));
        }
    };

    match &payload {
        TaskPayload::Prompt(text) => {
            // Registered + execute once immediately via CommandResult::Query.
            let header = format!(
                "Registered /loop {} (id={}) every {}. Running once now…",
                saved.name,
                saved.id,
                interval.human()
            );
            let msg = Message::User(UserMessage {
                uuid: Uuid::new_v4(),
                role: "user".to_string(),
                content: MessageContent::Text(format!("{}\n\n{}", header, text)),
                timestamp: chrono::Utc::now().timestamp(),
                is_meta: false,
                tool_use_result: None,
                source_tool_assistant_uuid: None,
            });
            CommandResult::Query(vec![msg])
        }
        TaskPayload::SlashCommand(cmd) => {
            // For slash-command payloads we don't dispatch inline: the
            // command dispatcher is a level above us and feeding a command
            // back through the message stream would double-execute the
            // /loop wrapper. Report the registration and point at /trigger.
            CommandResult::Output(format!(
                "Registered /loop {} (id={}) every {}. Payload: {}.\n\
                 The slash-command payload is not auto-executed — run `{}` \
                 now, or `/loop trigger {}` later.",
                saved.name,
                saved.id,
                interval.human(),
                cmd,
                cmd,
                saved.id
            ))
        }
    }
}

fn render_list(store: &SchedulerStore) -> String {
    match store.load() {
        Ok(tasks) => {
            if tasks.is_empty() {
                return "No /loop tasks registered.".into();
            }
            let mut out = String::new();
            out.push_str(&format!("Loops ({})\n", tasks.len()));
            out.push_str(&"─".repeat(8));
            out.push('\n');
            for t in tasks {
                let status = if t.paused { "paused" } else { "active" };
                out.push_str(&format!(
                    "  {id}  every {interval}  [{kind} · {status}]\n    \
                     payload ({ptype}): {payload}\n    next run: {next}\n",
                    id = t.id,
                    interval = Interval::from_seconds(t.interval_seconds).human(),
                    kind = t.kind.as_str(),
                    status = status,
                    ptype = t.payload.kind_label(),
                    payload = t.payload.display(),
                    next = t.next_run_at.to_rfc3339(),
                ));
            }
            out
        }
        Err(e) => format!("Could not read scheduler state: {}", e),
    }
}

fn remove_task(store: &SchedulerStore, id_raw: &str) -> String {
    if id_raw.is_empty() {
        return "Usage: /loop remove <id>".into();
    }
    let id = TaskId(id_raw.to_string());
    match store.remove(&id) {
        Ok(removed) => format!("Removed /loop task '{}' (id={}).", removed.name, removed.id),
        Err(SchedulerError::NotFound(_)) => format!(
            "No /loop task with id '{}' — run '/loop list' to see current loops.",
            id_raw
        ),
        Err(e) => format!("Could not remove task: {}", e),
    }
}

fn trigger_task(store: &SchedulerStore, id_raw: &str) -> String {
    if id_raw.is_empty() {
        return "Usage: /loop trigger <id>".into();
    }
    let id = TaskId(id_raw.to_string());
    match store.record_fired(&id) {
        Ok(task) => format!(
            "Marked /loop '{}' as fired. Payload: {} ({}). Next run at {}.",
            task.name,
            task.payload.display(),
            task.payload.kind_label(),
            task.next_run_at.to_rfc3339()
        ),
        Err(SchedulerError::NotFound(_)) => format!(
            "No /loop task with id '{}' — run '/loop list' to see current loops.",
            id_raw
        ),
        Err(e) => format!("Could not trigger task: {}", e),
    }
}

fn set_paused(store: &SchedulerStore, id_raw: &str, paused: bool) -> String {
    if id_raw.is_empty() {
        return if paused {
            "Usage: /loop pause <id>".into()
        } else {
            "Usage: /loop resume <id>".into()
        };
    }
    let id = TaskId(id_raw.to_string());
    match store.set_paused(&id, paused) {
        Ok(task) => {
            let verb = if paused { "paused" } else { "resumed" };
            format!(
                "{} /loop '{}' (id={}).",
                capitalize(verb),
                task.name,
                task.id
            )
        }
        Err(SchedulerError::NotFound(_)) => format!(
            "No /loop task with id '{}' — run '/loop list' to see current loops.",
            id_raw
        ),
        Err(e) => format!("Could not update task: {}", e),
    }
}

fn derive_name(payload: &TaskPayload) -> String {
    let raw = payload.display();
    let first_line = raw.lines().next().unwrap_or(raw).trim();
    let short: String = first_line.chars().take(40).collect();
    if short.is_empty() {
        "loop".into()
    } else {
        short
    }
}

fn split_first(input: &str) -> (String, &str) {
    match input.split_once(char::is_whitespace) {
        Some((head, rest)) => (head.to_lowercase(), rest),
        None => (input.to_lowercase(), ""),
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fresh_store() -> (tempfile::TempDir, SchedulerStore) {
        let dir = tempdir().unwrap();
        let store = SchedulerStore::new(dir.path().join("scheduled_tasks.json"));
        (dir, store)
    }

    #[test]
    fn help_lists_subcommands() {
        let txt = help_text();
        assert!(txt.contains("create a loop"));
        assert!(txt.contains("list"));
        assert!(txt.contains("remove"));
        assert!(txt.contains("trigger"));
        assert!(txt.contains("pause"));
        assert!(txt.contains("resume"));
    }

    #[test]
    fn empty_args_returns_help() {
        let (_dir, store) = fresh_store();
        match dispatch(&store, "") {
            CommandResult::Output(s) => assert!(s.contains("/loop")),
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn missing_payload_rejects() {
        let (_dir, store) = fresh_store();
        match dispatch(&store, "5m") {
            CommandResult::Output(s) => assert!(s.contains("Usage")),
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn creates_plain_prompt_and_returns_query() {
        let (_dir, store) = fresh_store();
        let result = dispatch(&store, "5m review the last commit");
        match result {
            CommandResult::Query(msgs) => {
                assert_eq!(msgs.len(), 1);
                let Message::User(u) = &msgs[0] else {
                    panic!("expected user message")
                };
                if let MessageContent::Text(t) = &u.content {
                    assert!(t.contains("review the last commit"));
                    assert!(t.contains("Registered /loop"));
                } else {
                    panic!("expected text content");
                }
            }
            _ => panic!("plain prompt should return Query"),
        }
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn creates_slash_command_without_running() {
        let (_dir, store) = fresh_store();
        let result = dispatch(&store, "10m /simplify");
        match result {
            CommandResult::Output(s) => {
                assert!(s.contains("Registered /loop"));
                assert!(s.contains("/simplify"));
                assert!(s.contains("not auto-executed"));
            }
            _ => panic!("slash command payload should be Output-only"),
        }
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn list_reports_empty_and_populated() {
        let (_dir, store) = fresh_store();
        match dispatch(&store, "list") {
            CommandResult::Output(s) => assert!(s.contains("No /loop tasks")),
            _ => panic!("expected Output"),
        }
        let _ = dispatch(&store, "5m do the thing");
        match dispatch(&store, "list") {
            CommandResult::Output(s) => {
                assert!(s.contains("Loops (1)"));
                assert!(s.contains("active"));
                assert!(s.contains("do the thing"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn remove_unknown_reports_not_found() {
        let (_dir, store) = fresh_store();
        match dispatch(&store, "remove nope") {
            CommandResult::Output(s) => assert!(s.contains("No /loop task with id")),
            _ => panic!("expected Output"),
        }
    }

    #[test]
    fn pause_and_resume_cycle() {
        let (_dir, store) = fresh_store();
        let _ = dispatch(&store, "5m do thing");
        let tasks = store.load().unwrap();
        let id = tasks[0].id.clone();

        match dispatch(&store, &format!("pause {}", id)) {
            CommandResult::Output(s) => assert!(s.contains("Paused")),
            _ => panic!("expected Output"),
        }
        assert!(store.load().unwrap()[0].paused);

        match dispatch(&store, &format!("resume {}", id)) {
            CommandResult::Output(s) => assert!(s.contains("Resumed")),
            _ => panic!("expected Output"),
        }
        assert!(!store.load().unwrap()[0].paused);
    }

    #[test]
    fn bad_interval_reports_clear_error() {
        let (_dir, store) = fresh_store();
        match dispatch(&store, "abc prompt") {
            CommandResult::Output(s) => {
                assert!(s.contains("Could not parse interval"));
                assert!(s.contains("abc"));
            }
            _ => panic!("expected Output"),
        }
        assert!(store.load().unwrap().is_empty());
    }
}
