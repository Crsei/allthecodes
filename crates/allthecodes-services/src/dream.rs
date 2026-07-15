//! Deterministic KAIROS dream-memory distillation.

use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryIndexEntry {
    pub date: NaiveDate,
    pub byte_size: usize,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DreamMemoryDocument {
    pub date: NaiveDate,
    pub byte_size: usize,
    pub markdown: String,
    pub truncated: bool,
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

/// List existing dream artifacts without triggering distillation.
pub fn list_dream_memories(preview_bytes: usize) -> Result<Vec<DreamMemoryIndexEntry>> {
    let dir = allthecodes_config::paths::memory_dir_global().join("dream");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::new();
    for entry in fs::read_dir(&dir)
        .with_context(|| format!("failed to read dream memory directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(date) = NaiveDate::parse_from_str(stem, "%Y-%m-%d") else {
            continue;
        };
        // Require canonical formatting so names such as 2026-7-1 are ignored.
        if stem != date.format("%Y-%m-%d").to_string() {
            continue;
        }
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to inspect dream memory: {}", path.display()))?;
        if !metadata.is_file() {
            continue;
        }
        let preview = public_dream_markdown(&read_bounded_utf8(&path, preview_bytes)?.0);
        entries.push(DreamMemoryIndexEntry {
            date,
            byte_size: usize::try_from(metadata.len()).unwrap_or(usize::MAX),
            preview: preview.split_whitespace().collect::<Vec<_>>().join(" "),
        });
    }
    entries.sort_by(|left, right| right.date.cmp(&left.date));
    Ok(entries)
}

/// Read one existing dream artifact with an explicit response cap. The route
/// layer parses the strict date first; this function derives the canonical path
/// and never accepts a raw path segment.
pub fn read_dream_memory(date: NaiveDate, max_bytes: usize) -> Result<Option<DreamMemoryDocument>> {
    let path = memory_path_for_date(date);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::Error::new(error).context(format!(
                "failed to inspect dream memory: {}",
                path.display()
            )));
        }
    };
    let (markdown, truncated) = read_bounded_utf8(&path, max_bytes)?;
    let markdown = public_dream_markdown(&markdown);
    Ok(Some(DreamMemoryDocument {
        date,
        byte_size: usize::try_from(metadata.len()).unwrap_or(usize::MAX),
        markdown,
        truncated,
    }))
}

fn read_bounded_utf8(path: &std::path::Path, max_bytes: usize) -> Result<(String, bool)> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open dream memory: {}", path.display()))?;
    let read_limit = max_bytes.saturating_add(1);
    let mut bytes = Vec::with_capacity(read_limit.min(64 * 1024));
    file.take(u64::try_from(read_limit).unwrap_or(u64::MAX))
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read dream memory: {}", path.display()))?;
    let truncated = bytes.len() > max_bytes;
    bytes.truncate(max_bytes);
    while !bytes.is_empty() && std::str::from_utf8(&bytes).is_err() {
        bytes.pop();
    }
    let content = String::from_utf8(bytes).context("dream memory is not valid UTF-8")?;
    Ok((content, truncated))
}

fn public_dream_markdown(content: &str) -> String {
    let mut public = content
        .lines()
        .filter(|line| !line.trim_start().starts_with("Source:"))
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') && !public.is_empty() {
        public.push('\n');
    }
    public
}

fn local_noon(date: NaiveDate) -> chrono::DateTime<Local> {
    match Local.with_ymd_and_hms(date.year(), date.month(), date.day(), 12, 0, 0) {
        LocalResult::Single(value) => value,
        LocalResult::Ambiguous(earliest, _) => earliest,
        LocalResult::None => {
            let midnight = date.and_time(chrono::NaiveTime::MIN);
            Local
                .from_local_datetime(&midnight)
                .earliest()
                .unwrap_or_else(|| midnight.and_utc().with_timezone(&Local))
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

    #[test]
    #[serial]
    fn dream_index_is_newest_first_and_ignores_noncanonical_names() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let dream_dir = allthecodes_config::paths::memory_dir_global().join("dream");
        std::fs::create_dir_all(&dream_dir).unwrap();
        std::fs::write(
            dream_dir.join("2026-07-15.md"),
            "# Older\n\nSource: `/private/daily.log`\n\nolder detail\n",
        )
        .unwrap();
        std::fs::write(
            dream_dir.join("2026-07-16.md"),
            "# Newer\n\nSource: `/private/daily.log`\n\nnewer detail\n",
        )
        .unwrap();
        std::fs::write(dream_dir.join("2026-7-16.md"), "invalid name").unwrap();

        let entries = list_dream_memories(512).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].date.to_string(), "2026-07-16");
        assert_eq!(entries[1].date.to_string(), "2026-07-15");
        assert!(!entries[0].preview.contains("/private/"));
    }

    #[test]
    #[serial]
    fn dream_detail_is_bounded_utf8_and_hides_host_source_path() {
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("ALLTHECODES_HOME", home.path());
        let date = NaiveDate::from_ymd_opt(2026, 7, 16).unwrap();
        let path = memory_path_for_date(date);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "# Dream\n\nSource: `/private/source.log`\n\n你好世界 repeated content\n",
        )
        .unwrap();

        let document = read_dream_memory(date, 52).unwrap().unwrap();
        assert!(document.truncated);
        assert!(!document.markdown.contains("/private/source.log"));
        assert!(std::str::from_utf8(document.markdown.as_bytes()).is_ok());
        assert!(document.byte_size > document.markdown.len());
    }
}
