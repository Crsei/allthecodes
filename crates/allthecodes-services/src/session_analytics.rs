//! Session analytics — cross-session statistics for the `/insights` command.
//!
//! This service walks the persisted session directory, optionally loads the
//! bodies to tally token/cost usage, and produces a [`InsightsReport`] that
//! the command wrapper renders as text. It is completely read-only: nothing
//! here mutates sessions on disk.
//!
//! The analytics layer is intentionally thin. It does not batch API calls,
//! generate model summaries, or write files — those are policy choices left
//! to callers.

use std::collections::HashMap;

use anyhow::Result;

use allthecodes_session::storage::{self, SessionInfo};
use allthecodes_types::message::Message;

/// Filter + scoping controls for [`compute_insights`].
#[derive(Debug, Clone)]
pub struct InsightsFilter {
    pub scope: Scope,
    /// Inclusive lower bound (unix seconds) on `last_modified`.
    pub since: Option<i64>,
    /// Sessions with fewer than this many messages are skipped as "minimal".
    pub min_messages: usize,
    /// Number of largest sessions to surface in `largest_sessions`.
    pub top_n: usize,
    /// Whether to load each session to compute token / cost figures.
    /// Disable for a faster metadata-only sweep.
    pub include_usage: bool,
}

impl Default for InsightsFilter {
    fn default() -> Self {
        Self {
            scope: Scope::All,
            since: None,
            min_messages: 2,
            top_n: 5,
            include_usage: true,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Scope {
    /// All sessions on disk.
    All,
    /// Only sessions that belong to the same workspace as the supplied cwd.
    Workspace(std::path::PathBuf),
}

/// Lightweight descriptor for the top-N session list in a report.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub session_id: String,
    pub title: String,
    pub workspace_name: String,
    pub message_count: usize,
    pub last_modified: i64,
}

/// Aggregated statistics across the selected sessions.
#[derive(Debug, Clone, Default)]
pub struct InsightsReport {
    pub session_count: usize,
    /// How many sessions on disk were filtered out (scope/since/min_messages).
    pub filtered_out: usize,
    pub total_messages: u64,
    pub total_user_messages: u64,
    pub total_assistant_messages: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    pub total_cost_usd: f64,
    /// Oldest `last_modified` among included sessions.
    pub oldest: Option<i64>,
    /// Newest `last_modified` among included sessions.
    pub newest: Option<i64>,
    /// Sorted by session count descending.
    pub workspace_breakdown: Vec<(String, usize)>,
    /// Sorted by message_count descending.
    pub largest_sessions: Vec<SessionSummary>,
    /// True when `filter.include_usage` was honored — false when we skipped
    /// the body scan.
    pub usage_included: bool,
}

/// Compute aggregated analytics for every session that matches `filter`.
pub fn compute_insights(filter: &InsightsFilter) -> Result<InsightsReport> {
    let raw = match &filter.scope {
        Scope::All => storage::list_sessions()?,
        Scope::Workspace(cwd) => storage::list_workspace_sessions(cwd)?,
    };

    let total_scanned = raw.len();
    let included: Vec<SessionInfo> = raw.into_iter().filter(|s| passes(s, filter)).collect();

    let mut report = InsightsReport {
        usage_included: filter.include_usage,
        ..Default::default()
    };
    report.session_count = included.len();
    report.filtered_out = total_scanned.saturating_sub(included.len());

    // Workspace bucketing.
    let mut buckets: HashMap<String, usize> = HashMap::new();
    for s in &included {
        let label = if s.workspace_name.is_empty() {
            s.cwd.clone()
        } else {
            s.workspace_name.clone()
        };
        *buckets.entry(label).or_insert(0) += 1;
    }
    let mut breakdown: Vec<(String, usize)> = buckets.into_iter().collect();
    breakdown.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    report.workspace_breakdown = breakdown;

    for s in &included {
        report.total_messages += s.message_count as u64;
        report.oldest = Some(match report.oldest {
            Some(prev) => prev.min(s.last_modified),
            None => s.last_modified,
        });
        report.newest = Some(match report.newest {
            Some(prev) => prev.max(s.last_modified),
            None => s.last_modified,
        });
    }

    if filter.include_usage {
        for s in &included {
            if let Ok(messages) = storage::load_session(&s.session_id) {
                accumulate_usage(&messages, &mut report);
            }
        }
    }

    // Largest sessions (by message_count then last_modified).
    let mut sorted = included;
    sorted.sort_by(|a, b| {
        b.message_count
            .cmp(&a.message_count)
            .then_with(|| b.last_modified.cmp(&a.last_modified))
    });
    report.largest_sessions = sorted
        .into_iter()
        .take(filter.top_n)
        .map(|s| SessionSummary {
            session_id: s.session_id,
            title: if s.title.is_empty() {
                "(untitled)".into()
            } else {
                s.title
            },
            workspace_name: s.workspace_name,
            message_count: s.message_count,
            last_modified: s.last_modified,
        })
        .collect();

    Ok(report)
}

fn passes(s: &SessionInfo, filter: &InsightsFilter) -> bool {
    if s.message_count < filter.min_messages {
        return false;
    }
    if let Some(since) = filter.since {
        if s.last_modified < since {
            return false;
        }
    }
    true
}

fn accumulate_usage(messages: &[Message], report: &mut InsightsReport) {
    for msg in messages {
        match msg {
            Message::User(u) => {
                // Don't count synthesized continuation messages (tool results).
                if u.is_meta || u.tool_use_result.is_some() {
                    continue;
                }
                report.total_user_messages += 1;
            }
            Message::Assistant(a) => {
                report.total_assistant_messages += 1;
                report.total_cost_usd += a.cost_usd;
                if let Some(usage) = &a.usage {
                    report.total_input_tokens += usage.input_tokens;
                    report.total_output_tokens += usage.output_tokens;
                    report.total_cache_read_tokens += usage.cache_read_input_tokens;
                    report.total_cache_creation_tokens += usage.cache_creation_input_tokens;
                }
            }
            _ => {}
        }
    }
}

/// Render an [`InsightsReport`] as a terminal-friendly text block.
///
/// The caller decides whether to send the text to the user, stash it in a
/// report artifact, etc.; this function deliberately has no side effects.
pub fn format_report(report: &InsightsReport, label: &str) -> String {
    let mut lines = Vec::new();
    lines.push(label.to_string());
    lines.push(String::new());
    lines.push(format!("  Sessions included:   {}", report.session_count));
    lines.push(format!("  Sessions filtered:   {}", report.filtered_out));
    lines.push(format!("  Total messages:      {}", report.total_messages));
    lines.push(format!(
        "  User turns:          {}",
        report.total_user_messages
    ));
    lines.push(format!(
        "  Assistant turns:     {}",
        report.total_assistant_messages
    ));

    if let (Some(oldest), Some(newest)) = (report.oldest, report.newest) {
        lines.push(format!(
            "  Activity span:       {} → {}",
            fmt_ts(oldest),
            fmt_ts(newest)
        ));
    }

    if report.usage_included {
        lines.push(String::new());
        lines.push("  Token usage:".into());
        lines.push(format!(
            "    Input:             {}",
            thousands(report.total_input_tokens)
        ));
        lines.push(format!(
            "    Output:            {}",
            thousands(report.total_output_tokens)
        ));
        lines.push(format!(
            "    Cache read:        {}",
            thousands(report.total_cache_read_tokens)
        ));
        lines.push(format!(
            "    Cache creation:    {}",
            thousands(report.total_cache_creation_tokens)
        ));
        lines.push(format!(
            "  Estimated cost:      {}",
            fmt_cost(report.total_cost_usd)
        ));
    } else {
        lines.push(String::new());
        lines.push("  (Usage stats skipped — rerun without --fast to load session bodies.)".into());
    }

    if !report.workspace_breakdown.is_empty() {
        lines.push(String::new());
        lines.push("  Sessions by workspace:".into());
        for (name, count) in report.workspace_breakdown.iter().take(8) {
            lines.push(format!("    {:<30} {}", truncate_inline(name, 30), count));
        }
        if report.workspace_breakdown.len() > 8 {
            lines.push(format!(
                "    ... and {} more",
                report.workspace_breakdown.len() - 8
            ));
        }
    }

    if !report.largest_sessions.is_empty() {
        lines.push(String::new());
        lines.push("  Largest sessions:".into());
        for s in &report.largest_sessions {
            let ts = chrono::DateTime::from_timestamp(s.last_modified, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "unknown".into());
            lines.push(format!(
                "    [{}] {:>4} msgs  {}  {}",
                &s.session_id.chars().take(8).collect::<String>(),
                s.message_count,
                ts,
                truncate_inline(&s.title, 50),
            ));
        }
    }

    lines.join("\n")
}

fn fmt_ts(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| format!("{}s", ts))
}

fn fmt_cost(usd: f64) -> String {
    if usd < 0.01 {
        format!("${:.4}", usd)
    } else {
        format!("${:.2}", usd)
    }
}

fn thousands(n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn truncate_inline(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let shortened: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", shortened)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_session::storage::{SerializableMessage, SessionFile};
    use std::path::{Path, PathBuf};

    struct HomeGuard {
        previous: Option<String>,
    }

    impl HomeGuard {
        fn set(path: &Path) -> Self {
            let previous = std::env::var("ALLTHECODES_HOME").ok();
            std::env::set_var("ALLTHECODES_HOME", path);
            Self { previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("ALLTHECODES_HOME", v),
                None => std::env::remove_var("ALLTHECODES_HOME"),
            }
        }
    }

    fn user_sm(uuid: &str) -> SerializableMessage {
        SerializableMessage {
            msg_type: "user".into(),
            uuid: uuid.into(),
            timestamp: 1_700_000_000,
            data: serde_json::json!({ "content": "hi", "is_meta": false }),
        }
    }

    fn assistant_sm(
        uuid: &str,
        input_tokens: u64,
        output_tokens: u64,
        cost: f64,
    ) -> SerializableMessage {
        SerializableMessage {
            msg_type: "assistant".into(),
            uuid: uuid.into(),
            timestamp: 1_700_000_000,
            data: serde_json::json!({
                "content": [],
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "cache_read_input_tokens": 0u64,
                    "cache_creation_input_tokens": 0u64,
                },
                "stop_reason": "end_turn",
                "cost_usd": cost,
            }),
        }
    }

    fn write_session(id: &str, cwd: &str, last_modified: i64, messages: Vec<SerializableMessage>) {
        let file = SessionFile {
            session_id: id.into(),
            created_at: last_modified,
            last_modified,
            cwd: cwd.into(),
            custom_title: None,
            chat_mode_override: None,
            messages,
        };
        std::fs::create_dir_all(storage::get_session_dir()).unwrap();
        let json = serde_json::to_string_pretty(&file).unwrap();
        std::fs::write(storage::get_session_file(id), json).unwrap();
    }

    #[test]
    #[serial_test::serial]
    fn compute_insights_aggregates_across_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_session(
            "big",
            "/proj-a",
            1_700_000_100,
            vec![
                user_sm("11111111-0000-0000-0000-000000000001"),
                assistant_sm("11111111-0000-0000-0000-000000000002", 100, 50, 0.01),
                user_sm("11111111-0000-0000-0000-000000000003"),
                assistant_sm("11111111-0000-0000-0000-000000000004", 200, 80, 0.02),
            ],
        );
        write_session(
            "small",
            "/proj-b",
            1_700_000_200,
            vec![
                user_sm("22222222-0000-0000-0000-000000000001"),
                assistant_sm("22222222-0000-0000-0000-000000000002", 10, 5, 0.001),
            ],
        );
        // Below min_messages — should be filtered out.
        write_session(
            "tiny",
            "/proj-a",
            1_700_000_050,
            vec![user_sm("33333333-0000-0000-0000-000000000001")],
        );

        let report = compute_insights(&InsightsFilter::default()).unwrap();
        assert_eq!(report.session_count, 2);
        assert_eq!(report.filtered_out, 1);
        assert_eq!(report.total_messages, 6);
        assert_eq!(report.total_input_tokens, 310);
        assert_eq!(report.total_output_tokens, 135);
        assert!((report.total_cost_usd - 0.031).abs() < 1e-9);
        assert_eq!(report.total_user_messages, 3);
        assert_eq!(report.total_assistant_messages, 3);
        assert_eq!(report.oldest, Some(1_700_000_100));
        assert_eq!(report.newest, Some(1_700_000_200));
        assert_eq!(report.largest_sessions.len(), 2);
        assert_eq!(report.largest_sessions[0].session_id, "big");
    }

    #[test]
    #[serial_test::serial]
    fn compute_insights_workspace_scope_filters_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let repo = temp.path().join("repo-x");
        std::fs::create_dir_all(&repo).unwrap();
        git2::Repository::init(&repo).unwrap();

        write_session(
            "in-repo",
            repo.to_string_lossy().as_ref(),
            1_700_000_100,
            vec![
                user_sm("44444444-0000-0000-0000-000000000001"),
                assistant_sm("44444444-0000-0000-0000-000000000002", 1, 1, 0.0),
            ],
        );
        write_session(
            "elsewhere",
            "/proj-c",
            1_700_000_200,
            vec![
                user_sm("55555555-0000-0000-0000-000000000001"),
                assistant_sm("55555555-0000-0000-0000-000000000002", 1, 1, 0.0),
            ],
        );

        let filter = InsightsFilter {
            scope: Scope::Workspace(repo.clone()),
            ..Default::default()
        };
        let report = compute_insights(&filter).unwrap();
        assert_eq!(report.session_count, 1);
        assert_eq!(report.largest_sessions[0].session_id, "in-repo");
    }

    #[test]
    #[serial_test::serial]
    fn compute_insights_since_filter_drops_old_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_session(
            "old",
            "/p",
            1_000,
            vec![
                user_sm("66666666-0000-0000-0000-000000000001"),
                assistant_sm("66666666-0000-0000-0000-000000000002", 1, 1, 0.0),
            ],
        );
        write_session(
            "recent",
            "/p",
            2_000,
            vec![
                user_sm("77777777-0000-0000-0000-000000000001"),
                assistant_sm("77777777-0000-0000-0000-000000000002", 1, 1, 0.0),
            ],
        );

        let filter = InsightsFilter {
            since: Some(1_500),
            ..Default::default()
        };
        let report = compute_insights(&filter).unwrap();
        assert_eq!(report.session_count, 1);
        assert_eq!(report.largest_sessions[0].session_id, "recent");
    }

    #[test]
    #[serial_test::serial]
    fn compute_insights_fast_mode_skips_usage() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_session(
            "any",
            "/p",
            1_000,
            vec![
                user_sm("88888888-0000-0000-0000-000000000001"),
                assistant_sm("88888888-0000-0000-0000-000000000002", 99, 99, 1.0),
            ],
        );

        let filter = InsightsFilter {
            include_usage: false,
            ..Default::default()
        };
        let report = compute_insights(&filter).unwrap();
        assert_eq!(report.session_count, 1);
        assert_eq!(report.total_input_tokens, 0);
        assert_eq!(report.total_cost_usd, 0.0);
        assert!(!report.usage_included);
    }

    #[test]
    fn format_report_contains_expected_sections() {
        let report = InsightsReport {
            session_count: 2,
            filtered_out: 3,
            total_messages: 10,
            total_user_messages: 4,
            total_assistant_messages: 4,
            total_input_tokens: 1_234,
            total_output_tokens: 5_678,
            total_cache_read_tokens: 0,
            total_cache_creation_tokens: 0,
            total_cost_usd: 0.12,
            oldest: Some(1_700_000_000),
            newest: Some(1_700_100_000),
            workspace_breakdown: vec![("proj".into(), 2)],
            largest_sessions: vec![SessionSummary {
                session_id: "abcdef1234".into(),
                title: "hello".into(),
                workspace_name: "proj".into(),
                message_count: 10,
                last_modified: 1_700_100_000,
            }],
            usage_included: true,
        };
        let text = format_report(&report, "Session insights (all workspaces)");
        assert!(text.contains("Session insights"));
        assert!(text.contains("Sessions included:   2"));
        assert!(text.contains("Token usage"));
        assert!(text.contains("Input:"));
        assert!(text.contains("Largest sessions"));
        assert!(text.contains("hello"));
    }

    #[test]
    fn thousands_formats_expected() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }

    #[test]
    fn truncate_inline_uses_ellipsis() {
        assert_eq!(truncate_inline("short", 10), "short");
        let out = truncate_inline(&"x".repeat(100), 5);
        assert_eq!(out.chars().count(), 5);
        assert!(out.ends_with("…"));
    }

    #[test]
    fn insights_filter_default_is_reasonable() {
        let f = InsightsFilter::default();
        assert!(matches!(f.scope, Scope::All));
        assert_eq!(f.min_messages, 2);
        assert!(f.include_usage);
        // Cover the path-helper to keep it from going dead.
        let _ = PathBuf::from(".");
    }
}
