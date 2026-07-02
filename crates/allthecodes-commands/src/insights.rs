//! /insights command -- session-history analytics and reporting.
//!
//! Usage:
//! - `/insights`                -- all sessions, with usage stats
//! - `/insights this`           -- only sessions in the current workspace
//! - `/insights recent [days]`  -- filter to last `days` (default 30)
//! - `/insights fast`           -- skip session-body scan for a quick metadata sweep
//!
//! The analytics helpers live in this crate so `/insights` does not depend on
//! root-private services while it is being migrated into `cc-commands`.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_services::cost_ledger;
use allthecodes_session::storage::{self, SessionInfo};
use allthecodes_types::message::Message;

/// Handler for the `/insights` slash command.
pub struct InsightsHandler;

#[async_trait]
impl CommandHandler for InsightsHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let parts: Vec<&str> = args.split_whitespace().collect();

        let mut filter = InsightsFilter::default();
        let mut label = String::from("Session insights (all workspaces)");

        match parts.as_slice() {
            [] => { /* defaults */ }
            ["this"] | ["workspace"] | ["ws"] => {
                filter.scope = Scope::Workspace(ctx.cwd.clone());
                label = format!("Session insights ({})", ctx.cwd.display());
            }
            ["fast"] => {
                filter.include_usage = false;
                label = "Session insights (fast — metadata only)".into();
            }
            ["recent", days_arg] => {
                let days = days_arg.parse::<i64>().map_err(|_| {
                    anyhow::anyhow!("expected a positive integer for days, got: {}", days_arg)
                })?;
                if days <= 0 {
                    return Ok(CommandResult::Output(
                        "'days' must be a positive integer.".into(),
                    ));
                }
                filter.since = Some(chrono::Utc::now().timestamp() - days * 86_400);
                label = format!("Session insights (last {} days)", days);
            }
            ["recent"] => {
                let days: i64 = 30;
                filter.since = Some(chrono::Utc::now().timestamp() - days * 86_400);
                label = format!("Session insights (last {} days)", days);
            }
            _ => {
                return Ok(CommandResult::Output(format!(
                    "Unknown /insights args: '{}'\n\n{}",
                    args.trim(),
                    USAGE_HELP
                )));
            }
        }

        let report = compute_insights(&filter)?;
        if report.session_count == 0 {
            return Ok(CommandResult::Output(
                "No sessions match the current filter. Try /insights or /session list.".into(),
            ));
        }
        Ok(CommandResult::Output(format_report(&report, &label)))
    }
}

const USAGE_HELP: &str = "Usage:\n  \
   /insights                     -- all sessions\n  \
   /insights this                -- current workspace only\n  \
   /insights recent [days]       -- last N days (default 30)\n  \
   /insights fast                -- metadata-only sweep (skips token/cost scan)";

/// Filter + scoping controls for [`compute_insights`].
#[derive(Debug, Clone)]
struct InsightsFilter {
    scope: Scope,
    /// Inclusive lower bound (unix seconds) on `last_modified`.
    since: Option<i64>,
    /// Sessions with fewer than this many messages are skipped as "minimal".
    min_messages: usize,
    /// Number of largest sessions to surface in `largest_sessions`.
    top_n: usize,
    /// Whether to load each session to compute token / cost figures.
    /// Disable for a faster metadata-only sweep.
    include_usage: bool,
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
enum Scope {
    /// All sessions on disk.
    All,
    /// Only sessions that belong to the same workspace as the supplied cwd.
    Workspace(std::path::PathBuf),
}

/// Lightweight descriptor for the top-N session list in a report.
#[derive(Debug, Clone, PartialEq)]
struct SessionSummary {
    session_id: String,
    title: String,
    workspace_name: String,
    message_count: usize,
    last_modified: i64,
}

/// Aggregated statistics across the selected sessions.
#[derive(Debug, Clone, Default)]
struct InsightsReport {
    session_count: usize,
    /// How many sessions on disk were filtered out (scope/since/min_messages).
    filtered_out: usize,
    total_messages: u64,
    total_user_messages: u64,
    total_assistant_messages: u64,
    total_input_tokens: u64,
    total_output_tokens: u64,
    total_cache_read_tokens: u64,
    total_cache_creation_tokens: u64,
    total_reasoning_output_tokens: u64,
    total_cost_usd: f64,
    api_calls: u64,
    unknown_pricing_count: u64,
    backfilled_count: u64,
    /// Oldest `last_modified` among included sessions.
    oldest: Option<i64>,
    /// Newest `last_modified` among included sessions.
    newest: Option<i64>,
    /// Sorted by session count descending.
    workspace_breakdown: Vec<(String, usize)>,
    /// Sorted by message_count descending.
    largest_sessions: Vec<SessionSummary>,
    /// True when `filter.include_usage` was honored; false when we skipped
    /// the body scan.
    usage_included: bool,
}

fn compute_insights(filter: &InsightsFilter) -> Result<InsightsReport> {
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
                accumulate_session_usage(&s.session_id, &messages, &mut report);
            }
        }
    }

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

fn accumulate_session_usage(session_id: &str, messages: &[Message], report: &mut InsightsReport) {
    for msg in messages {
        match msg {
            Message::User(u) => {
                if u.is_meta || u.tool_use_result.is_some() {
                    continue;
                }
                report.total_user_messages += 1;
            }
            Message::Assistant(_) => {
                report.total_assistant_messages += 1;
            }
            _ => {}
        }
    }

    let summary = cost_ledger::get_session_cost_summary(session_id, messages);
    report.total_input_tokens = report
        .total_input_tokens
        .saturating_add(summary.total_input_tokens);
    report.total_output_tokens = report
        .total_output_tokens
        .saturating_add(summary.total_output_tokens);
    report.total_cache_read_tokens = report
        .total_cache_read_tokens
        .saturating_add(summary.total_cache_read_tokens);
    report.total_cache_creation_tokens = report
        .total_cache_creation_tokens
        .saturating_add(summary.total_cache_creation_tokens);
    report.total_reasoning_output_tokens = report
        .total_reasoning_output_tokens
        .saturating_add(summary.total_reasoning_output_tokens);
    report.total_cost_usd += summary.total_cost_usd;
    report.api_calls = report.api_calls.saturating_add(summary.api_calls);
    report.unknown_pricing_count = report
        .unknown_pricing_count
        .saturating_add(summary.unknown_pricing_count);
    report.backfilled_count = report
        .backfilled_count
        .saturating_add(summary.backfilled_count);
}

fn format_report(report: &InsightsReport, label: &str) -> String {
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
            "  Activity span:       {} -> {}",
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
        if report.total_reasoning_output_tokens > 0 {
            lines.push(format!(
                "    Reasoning output:  {}",
                thousands(report.total_reasoning_output_tokens)
            ));
        }
        lines.push(format!(
            "  API calls:           {}",
            thousands(report.api_calls)
        ));
        lines.push(format!(
            "  Estimated cost:      {}",
            fmt_cost(report.total_cost_usd)
        ));
        if report.backfilled_count > 0 || report.unknown_pricing_count > 0 {
            lines.push(format!(
                "  Cost quality:        backfilled={} unknown_pricing={}",
                thousands(report.backfilled_count),
                thousands(report.unknown_pricing_count)
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_session::storage::{self, SerializableMessage, SessionFile};
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

    fn test_ctx(cwd: PathBuf) -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd,
            app_state: Default::default(),
            session_id: SessionId::from_string("curr"),
        }
    }

    fn write_session(id: &str, cwd: &str, last_modified: i64) {
        let messages = vec![
            SerializableMessage {
                msg_type: "user".into(),
                uuid: format!("{:0>8}-0000-0000-0000-000000000001", id),
                timestamp: last_modified,
                data: serde_json::json!({ "content": "hi", "is_meta": false }),
            },
            SerializableMessage {
                msg_type: "assistant".into(),
                uuid: format!("{:0>8}-0000-0000-0000-000000000002", id),
                timestamp: last_modified,
                data: serde_json::json!({
                    "content": [],
                    "usage": {
                        "input_tokens": 10u64,
                        "output_tokens": 5u64,
                        "cache_read_input_tokens": 0u64,
                        "cache_creation_input_tokens": 0u64,
                    },
                    "stop_reason": "end_turn",
                    "cost_usd": 0.001,
                }),
            },
        ];
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
        std::fs::write(
            storage::get_session_file(id),
            serde_json::to_string_pretty(&file).unwrap(),
        )
        .unwrap();
    }

    fn write_cost_event(session_id: &str, cost_event_data: serde_json::Value) {
        let dir = allthecodes_config::paths::runs_dir(session_id);
        std::fs::create_dir_all(&dir).unwrap();
        let event = serde_json::json!({
            "event_id": format!("evt_{session_id}"),
            "ts": "2026-01-01T00:00:00Z",
            "session_id": session_id,
            "source": "tui",
            "kind": "cost_recorded",
            "stage": "cost",
            "level": "info",
            "outcome": "completed",
            "data": cost_event_data,
        });
        std::fs::write(
            dir.join("events.ndjson"),
            format!("{}\n", serde_json::to_string(&event).unwrap()),
        )
        .unwrap();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_insights_no_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx(temp.path().to_path_buf());
        let result = InsightsHandler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(t) => assert!(t.contains("No sessions")),
            _ => panic!(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_insights_all_scope_counts_every_session() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_session("aa", "/p1", 1_700_000_100);
        write_session("bb", "/p2", 1_700_000_200);

        let mut ctx = test_ctx(PathBuf::from("/some/where"));
        let result = InsightsHandler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Session insights (all workspaces)"));
                assert!(text.contains("Sessions included:   2"));
                assert!(text.contains("Token usage"));
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_insights_prefers_runtime_cost_ledger() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_session("cc", "/p1", 1_700_000_100);
        write_cost_event(
            "cc",
            serde_json::json!({
                "schema_version": 1,
                "session_id": "cc",
                "message_id": "000000cc-0000-0000-0000-000000000002",
                "provider": "anthropic",
                "backend": "native",
                "model": "unknown-live-model",
                "pricing": {
                    "source": "unknown",
                    "matched_key": "",
                    "currency": "USD",
                    "input_per_1m": 0.0,
                    "output_per_1m": 0.0,
                    "cache_read_multiplier": 0.1,
                    "cache_creation_multiplier": 1.25
                },
                "usage": {
                    "input_tokens": 1000u64,
                    "output_tokens": 25u64,
                    "reasoning_output_tokens": 7u64,
                    "cache_read_input_tokens": 3u64,
                    "cache_creation_input_tokens": 2u64
                },
                "cost_usd": 0.5,
                "stop_reason": "end_turn",
                "is_retry": false,
                "attempt": 1,
                "backfilled": false
            }),
        );

        let mut ctx = test_ctx(PathBuf::from("/some/where"));
        let result = InsightsHandler.execute("", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Input:             1,000"));
                assert!(text.contains("Output:            25"));
                assert!(text.contains("Reasoning output:  7"));
                assert!(text.contains("Estimated cost:      $0.50"));
                assert!(text.contains("unknown_pricing=1"));
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_insights_recent_rejects_bad_days() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        let mut ctx = test_ctx(temp.path().to_path_buf());
        match InsightsHandler.execute("recent abc", &mut ctx).await {
            Ok(_) => panic!("expected an error for non-numeric days"),
            Err(e) => {
                let msg = format!("{}", e);
                assert!(msg.contains("days"), "got: {}", msg);
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn test_insights_fast_skips_usage() {
        let temp = tempfile::tempdir().unwrap();
        let _g = HomeGuard::set(temp.path());

        write_session("fast1", "/p1", 1_700_000_100);

        let mut ctx = test_ctx(temp.path().to_path_buf());
        let result = InsightsHandler.execute("fast", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("fast"));
                assert!(text.contains("Usage stats skipped"));
            }
            _ => panic!(),
        }
    }

    #[tokio::test]
    async fn test_insights_unknown_subcommand_shows_help() {
        let mut ctx = test_ctx(PathBuf::from("/x"));
        let result = InsightsHandler
            .execute("nonsense foo", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Unknown /insights args"));
                assert!(text.contains("Usage:"));
            }
            _ => panic!(),
        }
    }
}
