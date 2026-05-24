//! /audit-export command — export session as a verifiable audit record.
//!
//! Usage:
//!   /audit-export                — export current session to ~/.allthecodes/audits/
//!   /audit-export list           — list all audit export files
//!   /audit-export verify <path>  — verify integrity of an audit file
//!   /audit-export <path>         — export current session to a specific file
//!   /audit-export <session_id>   — export a saved session by ID

use anyhow::Result;
use async_trait::async_trait;

use crate::{CommandContext, CommandHandler, CommandResult};
use allthecodes_session::audit_export;

pub struct AuditExportHandler;

#[async_trait]
impl CommandHandler for AuditExportHandler {
    async fn execute(&self, args: &str, ctx: &mut CommandContext) -> Result<CommandResult> {
        let args = args.trim();

        if args.is_empty() {
            return export_current(ctx);
        }

        if args == "list" {
            return list_audit_files();
        }

        // /audit-export verify <path>
        if let Some(path_str) = args.strip_prefix("verify") {
            let path_str = path_str.trim();
            if path_str.is_empty() {
                return Ok(CommandResult::Output(
                    "Usage: /audit-export verify <path>".into(),
                ));
            }
            return verify_file(path_str);
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
    let path = audit_export::export_audit_messages(
        ctx.session_id.as_str(),
        &ctx.messages,
        &ctx.cwd.to_string_lossy(),
        None,
    )?;
    Ok(CommandResult::Output(format!(
        "Audit record exported to: {}\nUse `/audit-export verify {}` to verify integrity.",
        path.display(),
        path.display()
    )))
}

fn export_current_to_path(ctx: &CommandContext, path: &std::path::Path) -> Result<CommandResult> {
    let path = audit_export::export_audit_messages(
        ctx.session_id.as_str(),
        &ctx.messages,
        &ctx.cwd.to_string_lossy(),
        Some(path),
    )?;
    Ok(CommandResult::Output(format!(
        "Audit record exported to: {}",
        path.display()
    )))
}

fn list_audit_files() -> Result<CommandResult> {
    let files = audit_export::list_audits()?;
    if files.is_empty() {
        return Ok(CommandResult::Output(
            "No audit exports found. Use /audit-export to export the current session.".into(),
        ));
    }
    let mut out = format!("Audit exports ({}):\n", files.len());
    for f in &files {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        out.push_str(&format!("  {}\n", name));
    }
    Ok(CommandResult::Output(out))
}

fn verify_file(path_str: &str) -> Result<CommandResult> {
    let path = std::path::Path::new(path_str);
    let result = audit_export::verify_audit_file(path)?;

    let icon = if result.valid { "PASS" } else { "FAIL" };
    let mut out = format!("[{}] {}\n", icon, result.details);
    out.push_str(&format!("  Entries: {}\n", result.entry_count));
    if let Some(broken) = result.first_broken_at {
        out.push_str(&format!("  First broken at: entry #{}\n", broken));
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
            let path = audit_export::export_audit_record(&info.session_id, None)?;
            Ok(CommandResult::Output(format!(
                "Session {} audit record exported to: {}",
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test"),
            app_state: Default::default(),
            session_id: SessionId::from_string("test-session"),
        }
    }

    #[tokio::test]
    async fn test_audit_list_returns_output() {
        let handler = AuditExportHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("list", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(_) => {}
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_audit_verify_missing_path_shows_usage() {
        let handler = AuditExportHandler;
        let mut ctx = test_ctx();
        // "verify" with no path should return usage hint
        let result = handler.execute("verify", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("Usage"), "expected usage hint, got: {}", text);
            }
            _ => panic!("Expected Output"),
        }
    }

    #[tokio::test]
    async fn test_audit_nonexistent_session_id() {
        let handler = AuditExportHandler;
        let mut ctx = test_ctx();
        let result = handler
            .execute("00000000-0000-nonexistent-session-id99", &mut ctx)
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

    #[test]
    fn test_path_detection_json_extension() {
        let args = "output.json";
        assert!(args.ends_with(".json"));
    }

    #[test]
    fn test_path_detection_plain_id_not_path() {
        let args = "abc123deadbeef";
        assert!(!args.contains('/') && !args.contains('\\') && !args.ends_with(".json"));
    }
}
