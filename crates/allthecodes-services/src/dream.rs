//! Deterministic KAIROS dream-memory distillation.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::{Datelike, Duration, Local, LocalResult, NaiveDate, TimeZone};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamSummary {
    pub date: NaiveDate,
    pub source_log_path: PathBuf,
    pub line_count: usize,
    pub tasks: Vec<String>,
    pub decisions: Vec<String>,
    pub blockers: Vec<String>,
    pub follow_ups: Vec<String>,
}

pub fn distill_daily_log(date: NaiveDate) -> Result<DreamSummary> {
    let source_log_path = daily_log_path_for_date(date);
    let content = fs::read_to_string(&source_log_path).with_context(|| {
        format!(
            "failed to read daily log for dream distillation: {}",
            source_log_path.display()
        )
    })?;

    let mut summary = DreamSummary {
        date,
        source_log_path,
        line_count: content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
        tasks: Vec::new(),
        decisions: Vec::new(),
        blockers: Vec::new(),
        follow_ups: Vec::new(),
    };

    let mut seen_tasks = BTreeSet::new();
    let mut seen_decisions = BTreeSet::new();
    let mut seen_blockers = BTreeSet::new();
    let mut seen_follow_ups = BTreeSet::new();

    for line in content.lines() {
        let Some(item) = normalize_log_item(line) else {
            continue;
        };
        let lower = item.to_ascii_lowercase();

        if is_task_item(&lower) {
            push_unique(&mut summary.tasks, &mut seen_tasks, item.clone());
        }
        if is_decision_item(&lower) {
            push_unique(&mut summary.decisions, &mut seen_decisions, item.clone());
        }
        if is_blocker_item(&lower) {
            push_unique(&mut summary.blockers, &mut seen_blockers, item.clone());
        }
        if is_follow_up_item(&lower) {
            push_unique(&mut summary.follow_ups, &mut seen_follow_ups, item);
        }
    }

    Ok(summary)
}

pub fn write_memory(summary: &DreamSummary) -> Result<PathBuf> {
    let path = memory_path_for_date(summary.date);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create dream memory directory: {}",
                parent.display()
            )
        })?;
    }
    fs::write(&path, render_memory(summary))
        .with_context(|| format!("failed to write dream memory: {}", path.display()))?;
    Ok(path)
}

pub fn run_daily_dream_once(date: NaiveDate) -> Result<Option<PathBuf>> {
    let source_log_path = daily_log_path_for_date(date);
    if !source_log_path.exists() {
        return Ok(None);
    }

    let memory_path = memory_path_for_date(date);
    if memory_path.exists() {
        return Ok(None);
    }

    let summary = distill_daily_log(date)?;
    if summary.line_count == 0
        && summary.tasks.is_empty()
        && summary.decisions.is_empty()
        && summary.blockers.is_empty()
        && summary.follow_ups.is_empty()
    {
        return Ok(None);
    }

    write_memory(&summary).map(Some)
}

pub fn run_recent_days(days: u32, today: NaiveDate) -> Result<Vec<PathBuf>> {
    if days == 0 {
        bail!("days must be greater than zero")
    }

    let mut written = Vec::new();
    for offset in 0..days {
        let date = today - Duration::days(i64::from(offset));
        if let Some(path) = run_daily_dream_once(date)? {
            written.push(path);
        }
    }
    Ok(written)
}

pub fn daily_log_path_for_date(date: NaiveDate) -> PathBuf {
    allthecodes_config::paths::daily_log_path(local_noon(date))
}

pub fn memory_path_for_date(date: NaiveDate) -> PathBuf {
    allthecodes_config::paths::memory_dir_global()
        .join("dream")
        .join(format!("{date}.md"))
}

fn local_noon(date: NaiveDate) -> chrono::DateTime<Local> {
    match Local.with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(earliest, _) => earliest,
        LocalResult::None => {
            let midnight = date
                .and_hms_opt(0, 0, 0)
                .expect("valid NaiveDate should support midnight");
            Local
                .from_local_datetime(&midnight)
                .earliest()
                .expect("valid NaiveDate should map to local date")
        }
    }
}

fn normalize_log_item(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let item = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed)
        .trim();
    let item = item
        .strip_prefix("[ ] ")
        .or_else(|| item.strip_prefix("[x] "))
        .or_else(|| item.strip_prefix("[X] "))
        .unwrap_or(item)
        .trim();

    if item.is_empty() {
        None
    } else {
        Some(item.to_string())
    }
}

fn push_unique(items: &mut Vec<String>, seen: &mut BTreeSet<String>, item: String) {
    if seen.insert(item.clone()) {
        items.push(item);
    }
}

fn is_task_item(lower: &str) -> bool {
    lower.starts_with("task:")
        || lower.starts_with("todo:")
        || lower.starts_with("work:")
        || lower.contains(" task ")
        || lower.contains(" implement ")
}

fn is_decision_item(lower: &str) -> bool {
    lower.starts_with("decision:")
        || lower.starts_with("decided:")
        || lower.contains(" decision ")
        || lower.contains(" decided ")
}

fn is_blocker_item(lower: &str) -> bool {
    lower.starts_with("blocker:")
        || lower.starts_with("blocked:")
        || lower.starts_with("error:")
        || lower.starts_with("failed:")
        || lower.contains(" blocker ")
        || lower.contains(" blocked ")
        || lower.contains(" failed ")
}

fn is_follow_up_item(lower: &str) -> bool {
    lower.starts_with("follow-up:")
        || lower.starts_with("follow up:")
        || lower.starts_with("next:")
        || lower.starts_with("pending:")
        || lower.contains(" follow-up ")
        || lower.contains(" follow up ")
}

fn render_memory(summary: &DreamSummary) -> String {
    let mut output = String::new();
    output.push_str(&format!("# Dream Summary {}\n\n", summary.date));
    output.push_str(&format!(
        "Source: `{}`\n\n",
        summary.source_log_path.display()
    ));
    output.push_str(&format!("Line count: {}\n\n", summary.line_count));
    push_section(&mut output, "Tasks", &summary.tasks);
    push_section(&mut output, "Decisions", &summary.decisions);
    push_section(&mut output, "Blockers", &summary.blockers);
    push_section(&mut output, "Follow-ups", &summary.follow_ups);
    output
}

fn push_section(output: &mut String, title: &str, items: &[String]) {
    output.push_str(&format!("## {title}\n"));
    if items.is_empty() {
        output.push_str("- None captured.\n\n");
        return;
    }
    for item in items {
        output.push_str("- ");
        output.push_str(item);
        output.push('\n');
    }
    output.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Local, TimeZone};
    use serial_test::serial;

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::path::Path>) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value.as_ref());
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

    fn local_noon(date: NaiveDate) -> chrono::DateTime<Local> {
        Local
            .with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0)
            .single()
            .expect("test date should map to local time")
    }

    fn write_daily_log(date: NaiveDate, body: &str) {
        let path = allthecodes_config::paths::daily_log_path(local_noon(date));
        std::fs::create_dir_all(path.parent().expect("daily log has parent")).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    #[serial]
    fn distills_daily_log_and_writes_isolated_memory_file() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let date = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
        write_daily_log(
            date,
            r#"
## 2026-07-04

### 09:15
- Task: implement bridge sync worker parity.
- Decision: keep persistent paths under ALLTHECODES_HOME.
- Blocker: webhook secret was missing during validation.
- Follow-up: add dream memory scheduler hook.
"#,
        );

        let summary = distill_daily_log(date).unwrap();
        assert_eq!(summary.date, date);
        assert_eq!(
            summary.tasks,
            vec!["Task: implement bridge sync worker parity."]
        );
        assert_eq!(
            summary.decisions,
            vec!["Decision: keep persistent paths under ALLTHECODES_HOME."]
        );
        assert_eq!(
            summary.blockers,
            vec!["Blocker: webhook secret was missing during validation."]
        );
        assert_eq!(
            summary.follow_ups,
            vec!["Follow-up: add dream memory scheduler hook."]
        );

        let path = write_memory(&summary).unwrap();
        assert!(path.starts_with(home.path()));
        assert!(path.ends_with("memory/dream/2026-07-04.md"));
        let path_text = path.to_string_lossy();
        assert!(!path_text.contains(".Codex"));
        assert!(!path_text.contains(".claude"));

        let written = std::fs::read_to_string(path).unwrap();
        assert!(written.contains("# Dream Summary 2026-07-04"));
        assert!(written.contains("implement bridge sync worker parity"));
        assert!(written.contains("keep persistent paths under ALLTHECODES_HOME"));
        assert!(written.contains("add dream memory scheduler hook"));
    }

    #[test]
    #[serial]
    fn run_daily_dream_once_is_idempotent_for_same_date() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let date = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();
        write_daily_log(date, "- Task: distill once.\n");

        let first = run_daily_dream_once(date).unwrap();
        assert!(first.is_some());
        let second = run_daily_dream_once(date).unwrap();
        assert!(second.is_none());
    }

    #[test]
    #[serial]
    fn run_daily_dream_once_skips_missing_daily_log() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let date = NaiveDate::from_ymd_opt(2026, 7, 4).unwrap();

        assert!(run_daily_dream_once(date).unwrap().is_none());
    }
}
