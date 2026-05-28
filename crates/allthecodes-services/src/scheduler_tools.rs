use std::sync::Arc;

use anyhow::{bail, Result};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};

use allthecodes_engine::types::message::AssistantMessage;
use allthecodes_engine::types::tool::{
    PermissionResult, Tool, ToolProgress, ToolResult, ToolUseContext, ValidationResult,
};

use crate::scheduler::{
    parse_cron, parse_interval, ScheduleKind, ScheduledTask, SchedulerKind, SchedulerStore, TaskId,
    TaskPayload,
};

pub fn tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(CronCreateTool),
        Arc::new(CronDeleteTool),
        Arc::new(CronListTool),
    ]
}

pub struct CronCreateTool;
pub struct CronDeleteTool;
pub struct CronListTool;

#[derive(Debug)]
struct CronCreateInput {
    schedule: String,
    prompt: String,
    name: Option<String>,
    run_immediately: bool,
    timezone: Option<String>,
}

fn parse_create_input(input: &Value) -> Result<CronCreateInput> {
    let schedule = input
        .get("schedule")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("schedule is required"))?
        .to_string();
    let prompt = input
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("prompt is required"))?
        .to_string();
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let timezone = input
        .get("timezone")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    Ok(CronCreateInput {
        schedule,
        prompt,
        name,
        run_immediately: input
            .get("run_immediately")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        timezone,
    })
}

fn task_to_json(task: &ScheduledTask) -> Value {
    json!({
        "id": task.id.as_str(),
        "kind": task.kind.as_str(),
        "name": task.name,
        "schedule": task.schedule,
        "schedule_kind": match task.schedule_kind {
            ScheduleKind::Interval => "interval",
            ScheduleKind::Cron => "cron",
        },
        "timezone": task.timezone,
        "prompt": task.payload.display(),
        "payload_kind": task.payload.kind_label(),
        "created_at": task.created_at.to_rfc3339(),
        "last_run_at": task.last_run_at.map(|dt| dt.to_rfc3339()),
        "next_run_at": task.next_run_at.to_rfc3339(),
        "paused": task.paused,
    })
}

fn schedule_kind_and_next(schedule: &str) -> Result<(ScheduleKind, u64, chrono::DateTime<Utc>)> {
    let now = Utc::now();
    if schedule.split_whitespace().count() == 5 {
        let cron = parse_cron(schedule)?;
        let next = cron
            .next_after(now)
            .ok_or_else(|| anyhow::anyhow!("cron expression has no run time within 5 years"))?;
        return Ok((ScheduleKind::Cron, 60, next));
    }
    let interval = parse_interval(schedule)?;
    Ok((
        ScheduleKind::Interval,
        interval.seconds(),
        now + chrono::Duration::seconds(interval.seconds() as i64),
    ))
}

#[async_trait]
impl Tool for CronCreateTool {
    fn name(&self) -> &str {
        "CronCreate"
    }

    async fn description(&self, _input: &Value) -> String {
        "Create a persistent local scheduled prompt or slash command.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "schedule": {"type": "string", "description": "A 5-field cron expression or interval like 5m/1h."},
                "prompt": {"type": "string", "description": "Prompt or slash command to run when the schedule fires."},
                "name": {"type": "string"},
                "run_immediately": {"type": "boolean", "default": false},
                "timezone": {"type": "string", "description": "Reserved for future local-time cron evaluation."}
            },
            "required": ["schedule", "prompt"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        match parse_create_input(input)
            .and_then(|parsed| schedule_kind_and_next(&parsed.schedule).map(|_| ()))
        {
            Ok(()) => ValidationResult::Ok,
            Err(err) => ValidationResult::Error {
                message: err.to_string(),
                error_code: 400,
            },
        }
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        let schedule = input.get("schedule").and_then(Value::as_str).unwrap_or("");
        let prompt = input.get("prompt").and_then(Value::as_str).unwrap_or("");
        PermissionResult::Ask {
            message: format!(
                "Allow creating persistent cron job '{}' for prompt '{}'?",
                schedule,
                prompt.chars().take(120).collect::<String>()
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let parsed = parse_create_input(&input)?;
        let (kind, interval_seconds, next_run_at) = schedule_kind_and_next(&parsed.schedule)?;
        let now = Utc::now();
        let name = parsed
            .name
            .unwrap_or_else(|| parsed.prompt.chars().take(48).collect());
        let mut task = ScheduledTask::new(
            SchedulerKind::LocalCron,
            name,
            parsed.schedule.clone(),
            crate::scheduler::Interval::from_seconds(interval_seconds),
            TaskPayload::from_user_input(&parsed.prompt),
            now,
        );
        task.schedule_kind = kind;
        task.timezone = parsed.timezone;
        task.next_run_at = if parsed.run_immediately {
            now
        } else {
            next_run_at
        };

        let task = SchedulerStore::open_default().add(task)?;
        Ok(ToolResult {
            data: json!({"created": task_to_json(&task)}),
            model_content: Some(allthecodes_engine::types::message::ToolResultContent::Text(
                format!(
                    "Created cron job {}. Next run: {}",
                    task.id, task.next_run_at
                ),
            )),
            display_preview: Some(format!("Cron job {} created", task.id)),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Create a persistent local cron job. Use only when the user asks for scheduled or recurring work.".to_string()
    }
}

#[async_trait]
impl Tool for CronDeleteTool {
    fn name(&self) -> &str {
        "CronDelete"
    }

    async fn description(&self, _input: &Value) -> String {
        "Delete a persistent local cron job by id.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"id": {"type": "string"}},
            "required": ["id"]
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
    }

    async fn validate_input(&self, input: &Value, _ctx: &ToolUseContext) -> ValidationResult {
        if input
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            return ValidationResult::Error {
                message: "id is required".to_string(),
                error_code: 400,
            };
        }
        ValidationResult::Ok
    }

    async fn check_permissions(&self, input: &Value, _ctx: &ToolUseContext) -> PermissionResult {
        PermissionResult::Ask {
            message: format!(
                "Allow deleting cron job '{}'?",
                input.get("id").and_then(Value::as_str).unwrap_or("")
            ),
        }
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let id = input.get("id").and_then(Value::as_str).unwrap_or("").trim();
        if id.is_empty() {
            bail!("id is required");
        }
        let removed = SchedulerStore::open_default().remove(&TaskId(id.to_string()))?;
        Ok(ToolResult {
            data: json!({"deleted": task_to_json(&removed)}),
            model_content: Some(allthecodes_engine::types::message::ToolResultContent::Text(
                format!("Deleted cron job {}", removed.id),
            )),
            display_preview: Some(format!("Cron job {} deleted", removed.id)),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "Delete a persistent local cron job by id.".to_string()
    }
}

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str {
        "CronList"
    }

    async fn description(&self, _input: &Value) -> String {
        "List persistent local cron jobs.".to_string()
    }

    fn input_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"include_paused": {"type": "boolean", "default": true}}
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn call(
        &self,
        input: Value,
        _ctx: &ToolUseContext,
        _parent_message: &AssistantMessage,
        _on_progress: Option<Box<dyn Fn(ToolProgress) + Send + Sync>>,
    ) -> Result<ToolResult> {
        let include_paused = input
            .get("include_paused")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mut tasks = SchedulerStore::open_default().load()?;
        if !include_paused {
            tasks.retain(|task| !task.paused);
        }
        let data = json!({"jobs": tasks.iter().map(task_to_json).collect::<Vec<_>>()});
        let text = if tasks.is_empty() {
            "No cron jobs registered.".to_string()
        } else {
            tasks
                .iter()
                .map(|task| {
                    format!(
                        "{}\t{}\t{}\tnext {}",
                        task.id,
                        task.schedule,
                        task.payload.display(),
                        task.next_run_at
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(ToolResult {
            data,
            model_content: Some(allthecodes_engine::types::message::ToolResultContent::Text(
                text.clone(),
            )),
            display_preview: Some(text),
            new_messages: vec![],
        })
    }

    async fn prompt(&self) -> String {
        "List persistent local cron jobs.".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cron_and_interval_schedules() {
        assert_eq!(
            schedule_kind_and_next("*/5 * * * *").unwrap().0,
            ScheduleKind::Cron
        );
        assert_eq!(
            schedule_kind_and_next("5m").unwrap().0,
            ScheduleKind::Interval
        );
    }

    #[test]
    fn tool_names_are_stable() {
        let names = tools()
            .into_iter()
            .map(|tool| tool.name().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["CronCreate", "CronDelete", "CronList"]);
    }
}
