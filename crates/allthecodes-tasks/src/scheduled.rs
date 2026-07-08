use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SCHEDULED_TASKS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledAgentTask {
    pub id: String,
    pub prompt: String,
    pub cwd: String,
    pub schedule: ScheduleSpec,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_run_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ScheduleSpec {
    Interval { every_seconds: u64 },
    Once { run_at: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScheduledTasksFile {
    schema_version: u32,
    tasks: Vec<ScheduledAgentTask>,
}

pub fn scheduled_tasks_path() -> PathBuf {
    allthecodes_config::paths::data_root()
        .join("scheduled_tasks")
        .join("tasks.json")
}

pub fn load_scheduled_tasks() -> Result<Vec<ScheduledAgentTask>> {
    let path = scheduled_tasks_path();
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read scheduled tasks {}", path.display()))?;
    let file: ScheduledTasksFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse scheduled tasks {}", path.display()))?;
    Ok(file.tasks)
}

pub fn save_scheduled_tasks(tasks: &[ScheduledAgentTask]) -> Result<()> {
    let path = scheduled_tasks_path();
    let file = ScheduledTasksFile {
        schema_version: SCHEDULED_TASKS_SCHEMA_VERSION,
        tasks: tasks.to_vec(),
    };
    write_json_atomic(&path, &file)
}

pub fn claim_due_scheduled_tasks(now: DateTime<Utc>) -> Result<Vec<ScheduledAgentTask>> {
    let mut tasks = load_scheduled_tasks()?;
    let mut due = Vec::new();
    let now_string = now.to_rfc3339();

    for task in &mut tasks {
        if !task.enabled || task_due_at(task).is_none_or(|due_at| due_at > now) {
            continue;
        }
        due.push(task.clone());
        task.last_run_at = Some(now_string.clone());
        match next_run_after(&task.schedule, now) {
            Some(next) => task.next_run_at = Some(next.to_rfc3339()),
            None => {
                task.next_run_at = None;
                task.enabled = false;
            }
        }
    }

    if !due.is_empty() {
        save_scheduled_tasks(&tasks)?;
    }
    Ok(due)
}

fn task_due_at(task: &ScheduledAgentTask) -> Option<DateTime<Utc>> {
    task.next_run_at
        .as_deref()
        .or(match &task.schedule {
            ScheduleSpec::Once { run_at } => Some(run_at.as_str()),
            ScheduleSpec::Interval { .. } => None,
        })
        .and_then(parse_datetime)
}

fn next_run_after(schedule: &ScheduleSpec, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match schedule {
        ScheduleSpec::Interval { every_seconds } => {
            let seconds = (*every_seconds).max(1) as i64;
            Some(now + Duration::seconds(seconds))
        }
        ScheduleSpec::Once { .. } => None,
    }
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create scheduled tasks dir {}", parent.display()))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tasks.json"),
        uuid::Uuid::new_v4()
    ));
    let json = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, json)
        .with_context(|| format!("failed to write scheduled tasks temp {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| {
        format!(
            "failed to rename scheduled tasks temp {} to {}",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn task(id: &str, next_run_at: String, enabled: bool) -> ScheduledAgentTask {
        ScheduledAgentTask {
            id: id.to_string(),
            prompt: format!("run {id}"),
            cwd: "/repo".to_string(),
            schedule: ScheduleSpec::Interval { every_seconds: 60 },
            enabled,
            last_run_at: None,
            next_run_at: Some(next_run_at),
        }
    }

    #[test]
    #[serial_test::serial]
    fn due_scheduled_tasks_are_selected_once() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let now = Utc::now();
        save_scheduled_tasks(&[task("due", (now - Duration::seconds(1)).to_rfc3339(), true)])
            .unwrap();

        let due = claim_due_scheduled_tasks(now).unwrap();
        assert_eq!(
            due.iter().map(|task| task.id.as_str()).collect::<Vec<_>>(),
            vec!["due"]
        );

        let due_again = claim_due_scheduled_tasks(now).unwrap();
        assert!(due_again.is_empty());

        let stored = load_scheduled_tasks().unwrap();
        assert_eq!(
            stored[0].last_run_at.as_deref(),
            Some(now.to_rfc3339().as_str())
        );
        assert!(stored[0].next_run_at.is_some());
    }

    #[test]
    #[serial_test::serial]
    fn disabled_scheduled_tasks_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let now = Utc::now();
        save_scheduled_tasks(&[task(
            "disabled",
            (now - Duration::seconds(1)).to_rfc3339(),
            false,
        )])
        .unwrap();

        assert!(claim_due_scheduled_tasks(now).unwrap().is_empty());
    }

    #[test]
    #[serial_test::serial]
    fn scheduled_tasks_reload_from_allthecodes_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set_path("ALLTHECODES_HOME", tmp.path());
        let now = Utc::now();
        let saved = task("persisted", now.to_rfc3339(), true);

        save_scheduled_tasks(std::slice::from_ref(&saved)).unwrap();

        assert_eq!(
            scheduled_tasks_path(),
            tmp.path().join("scheduled_tasks").join("tasks.json")
        );
        let loaded = load_scheduled_tasks().unwrap();
        assert_eq!(loaded, vec![saved]);
    }
}
