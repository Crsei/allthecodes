//! /session-export command — export session as a structured JSON data package.
//!
//! Usage:
//!   /session-export              — export current session to ~/.allthecodes/exports/
//!   /session-export list         — list all session export files
//!   /session-export <path>       — export current session to a specific file
//!   /session-export <session_id> — export a saved session by ID

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_session::session_export;

pub struct SessionExportHandler;

#[async_trait]
impl CommandHandler for SessionExportHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();

        if args.is_empty() {
            return export_current(ctx);
        }

        if args == "list" {
            return list_exports();
        }

        // If it looks like a file path, export to that path
        if args.contains('/') || args.contains('\\') || args.ends_with(".json") {
            let path = std::path::Path::new(args);
            return export_current_to_path(ctx, path);
        }

        // Otherwise treat as session ID
        export_by_id(args)
    }
}

fn export_current(ctx: &CommandContext) -> Result<CommandResult> {
    let (path, export) = session_export::export_session(
        ctx.session_id.as_str(),
        &ctx.messages,
        &ctx.cwd.to_string_lossy(),
        None,
    )?;

    let summary = format_export_summary(&export);
    Ok(CommandResult::Output(format!(
        "Session exported to: {}\n\n{}",
        path.display(),
        summary
    )))
}

fn export_current_to_path(ctx: &CommandContext, path: &std::path::Path) -> Result<CommandResult> {
    let (path, _) = session_export::export_session(
        ctx.session_id.as_str(),
        &ctx.messages,
        &ctx.cwd.to_string_lossy(),
        Some(path),
    )?;
    Ok(CommandResult::Output(format!(
        "Session exported to: {}",
        path.display()
    )))
}

fn list_exports() -> Result<CommandResult> {
    let files = session_export::list_session_exports()?;
    if files.is_empty() {
        return Ok(CommandResult::Output(
            "No session exports found. Use /session-export to export the current session.".into(),
        ));
    }
    let mut out = format!("Session exports ({}):\n", files.len());
    for f in &files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        out.push_str(&format!("  {}\n", name));
    }
    Ok(CommandResult::Output(out))
}

fn export_by_id(session_id: &str) -> Result<CommandResult> {
    let sessions = allthecodes_session::storage::list_sessions()?;
    let matched = sessions
        .iter()
        .find(|s| s.session_id == session_id || s.session_id.starts_with(session_id));

    match matched {
        Some(info) => {
            let (path, _) = session_export::export_saved_session(&info.session_id, None)?;
            Ok(CommandResult::Output(format!(
                "Session {} exported to: {}",
                &info.session_id[..8],
                path.display()
            )))
        }
        None => Ok(CommandResult::Output(format!(
            "No session found matching '{}'. Use /session list to see available sessions.",
            session_id
        ))),
    }
}

/// Format a brief summary of the export for the user.
fn format_export_summary(export: &session_export::SessionExport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Messages: {}", export.transcript.message_count));
    lines.push(format!(
        "  User: {}, Assistant: {}, System: {}",
        export.transcript.user_message_count,
        export.transcript.assistant_message_count,
        export.transcript.system_message_count,
    ));
    lines.push(format!("Tool calls: {}", export.tool_calls.len()));
    if !export.context.unique_tools_used.is_empty() {
        lines.push(format!(
            "  Tools used: {}",
            export.context.unique_tools_used.join(", ")
        ));
    }
    lines.push(format!("API requests: {}", export.api_view.request_count));
    lines.push(format!(
        "Compactions: {}",
        export.compression.total_compactions
    ));
    if !export.compression.content_replacements.is_empty() {
        lines.push(format!(
            "Content replacements: {}",
            export.compression.content_replacements.len()
        ));
    }
    lines.push(format!(
        "Tokens: ~{} / {} ({:.1}%)",
        export.context.estimated_total_tokens,
        export.context.context_window_size,
        export.context.utilization_pct,
    ));
    if export.context.total_cost_usd > 0.0 {
        lines.push(format!("Cost: ${:.4}", export.context.total_cost_usd));
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_session::session_export::{
        ApiViewData, CompressionData, ContextSnapshot, SessionExport, SessionMeta, TranscriptData,
        SESSION_EXPORT_SCHEMA_VERSION,
    };
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    /// Build a minimal `SessionExport` for testing `format_export_summary`.
    #[allow(clippy::too_many_arguments)]
    fn minimal_export(
        msg_count: usize,
        user: usize,
        asst: usize,
        sys: usize,
        tool_count: usize,
        compactions: usize,
        tokens: u64,
        window: u64,
        cost: f64,
    ) -> SessionExport {
        SessionExport {
            schema_version: SESSION_EXPORT_SCHEMA_VERSION,
            exported_at: "2026-01-01T00:00:00Z".into(),
            session: SessionMeta {
                session_id: "test".into(),
                project_path: None,
                git_branch: None,
                git_head_sha: None,
                model: None,
                mode: None,
                permission_mode: None,
                custom_title: None,
                tags: Vec::new(),
                started_at: None,
                ended_at: None,
            },
            raw_transcript: TranscriptData {
                messages: vec![],
                message_count: msg_count,
                user_message_count: user,
                assistant_message_count: asst,
                system_message_count: sys,
            },
            transcript: TranscriptData {
                messages: vec![],
                message_count: msg_count,
                user_message_count: user,
                assistant_message_count: asst,
                system_message_count: sys,
            },
            api_view: ApiViewData::default(),
            api_requests: Vec::new(),
            tool_calls: vec![],
            compression: CompressionData {
                compact_boundaries: vec![],
                content_replacements: vec![],
                microcompact_replacements: vec![],
                total_compactions: compactions,
            },
            context: ContextSnapshot {
                estimated_total_tokens: tokens,
                context_window_size: window,
                utilization_pct: if window > 0 {
                    tokens as f64 / window as f64 * 100.0
                } else {
                    0.0
                },
                total_cost_usd: cost,
                total_input_tokens: 0,
                total_output_tokens: 0,
                cache_read_tokens: 0,
                api_call_count: 0,
                tool_use_count: tool_count,
                unique_tools_used: vec![],
            },
        }
    }

    #[test]
    fn test_format_export_summary_basic() {
        let export = minimal_export(5, 2, 2, 1, 3, 0, 1000, 200_000, 0.0);
        let summary = format_export_summary(&export);
        assert!(summary.contains("Messages: 5"));
        assert!(summary.contains("User: 2"));
        assert!(summary.contains("Assistant: 2"));
        assert!(summary.contains("System: 1"));
        assert!(summary.contains("Tool calls: 0")); // tool_calls vec is empty
        assert!(summary.contains("Compactions: 0"));
        assert!(summary.contains("Tokens: ~1000"));
    }

    #[test]
    fn test_format_export_summary_includes_cost_when_nonzero() {
        let export = minimal_export(1, 1, 0, 0, 0, 0, 500, 200_000, 0.0123);
        let summary = format_export_summary(&export);
        assert!(summary.contains("Cost: $0.0123"));
    }

    #[test]
    fn test_format_export_summary_no_cost_line_when_zero() {
        let export = minimal_export(1, 1, 0, 0, 0, 0, 500, 200_000, 0.0);
        let summary = format_export_summary(&export);
        assert!(!summary.contains("Cost:"));
    }

    #[test]
    fn test_format_export_summary_token_percentage() {
        // 10_000 tokens out of 200_000 = 5.0%
        let mut export = minimal_export(1, 1, 0, 0, 0, 0, 10_000, 200_000, 0.0);
        export.context.utilization_pct = 5.0;
        let summary = format_export_summary(&export);
        assert!(summary.contains("5.0%"), "summary: {}", summary);
    }

    #[tokio::test]
    async fn test_session_export_list_returns_output() {
        let handler = SessionExportHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("list", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(_) => {}
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_session_export_nonexistent_session_id() {
        let handler = SessionExportHandler;
        let mut ctx = test_ctx();
        let result = handler
            .execute("00000000-nonexistent-session-9999999", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(
                    text.contains("No session found"),
                    "expected not-found message, got: {}",
                    text
                );
            }
            _ => panic!("Expected Output"),
        }
    }
}
