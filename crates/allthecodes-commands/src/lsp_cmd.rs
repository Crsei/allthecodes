//! `/lsp` command -- LSP server status cards, recommendation settings,
//! and project-based LSP plugin recommendations.

use anyhow::Result;
use async_trait::async_trait;

use super::{CommandContext, CommandHandler, CommandResult};
use allthecodes_ipc_protocol::subsystem_types::LspServerInfo;

pub struct LspHandler;

#[async_trait]
impl CommandHandler for LspHandler {
    async fn execute(&self, args: &str, _ctx: &mut CommandContext) -> Result<CommandResult> {
        let mut parts = args.split_whitespace();
        let sub = parts.next().unwrap_or("status");
        match sub {
            "" | "status" | "servers" => Ok(CommandResult::Output(render_status())),
            "recommendations" | "recommendation" => {
                let flag_all = parts.any(|p| p == "--all");
                Ok(CommandResult::Output(render_recommendations(flag_all)))
            }
            "recommend" => {
                let language = parts.next();
                Ok(CommandResult::Output(render_project_recommendations(
                    language,
                )))
            }
            "help" | "?" => Ok(CommandResult::Output(usage())),
            other => Ok(CommandResult::Output(format!(
                "Unknown /lsp subcommand: '{other}'\n\n{}",
                usage()
            ))),
        }
    }
}

fn render_status() -> String {
    let servers = crate::runtime::lsp_server_info_list();
    let mut lines = vec!["LSP server status".to_string()];
    if servers.is_empty() {
        lines.push("No LSP servers configured.".to_string());
    } else {
        for server in servers {
            lines.push(render_server_card(&server));
        }
    }
    lines.push(String::new());
    lines.push("Actions: /lsp recommendations, SystemStatus {\"subsystem\":\"lsp\"}".to_string());
    lines.join("\n")
}

fn render_server_card(server: &LspServerInfo) -> String {
    let mut card = format!(
        "\n[{}]\n  state: {}\n  open files: {}\n  extensions: {}",
        server.language_id,
        server.state,
        server.open_files_count,
        if server.extensions.is_empty() {
            "(none)".to_string()
        } else {
            server.extensions.join(", ")
        }
    );
    if let Some(error) = &server.error {
        card.push_str(&format!("\n  error: {error}"));
    }
    card
}

fn render_recommendations(show_all: bool) -> String {
    let settings = crate::runtime::lsp_recommendation_settings();
    let muted = if settings.muted_plugins.is_empty() {
        "(none)".to_string()
    } else {
        settings.muted_plugins.join(", ")
    };
    let mut lines = vec![format!(
        "LSP recommendation settings\n  disabled: {}\n  muted plugins: {}\n",
        settings.disabled, muted,
    )];

    if show_all {
        let recs = crate::runtime::lsp_recommendations();
        if recs.is_empty() {
            lines.push(
                "No active recommendations (recommendation engine not wired or no project detected)."
                    .to_string(),
            );
        } else {
            lines.push("All recommendations:".to_string());
            for rec in &recs {
                let status = if rec.is_dismissed {
                    "dismissed"
                } else if rec.is_already_installed {
                    "installed"
                } else {
                    "new"
                };
                lines.push(format!(
                    "  [{status}] {} ({}) — {}",
                    rec.plugin_name,
                    rec.languages.join(", "),
                    rec.description,
                ));
            }
        }
        lines.push(String::new());
        lines.push("Use `/lsp recommend <language>` for focused recommendations.".to_string());
    }

    lines.push(
        "Recommendations are shown when the backend emits a real LSP RecommendationRequest."
            .to_string(),
    );
    lines.join("\n")
}

fn render_project_recommendations(language: Option<&str>) -> String {
    let settings = crate::runtime::lsp_recommendation_settings();
    if settings.disabled {
        return "LSP recommendations are disabled. Run `/lsp` to re-enable.".to_string();
    }

    let recs = crate::runtime::lsp_recommendations();
    let filtered: Vec<_> = match language {
        Some(lang) => recs
            .into_iter()
            .filter(|r| r.languages.iter().any(|l| l.eq_ignore_ascii_case(lang)))
            .collect(),
        None => recs,
    };

    if filtered.is_empty() {
        let lang_msg = language
            .map(|l| format!(" for language '{l}'"))
            .unwrap_or_default();
        return format!(
            "No LSP plugin recommendations{lang_msg}.\n\
             The recommendation engine may not be wired yet, or no matching\n\
             project files were detected.\n\n\
             Try running `/lsp recommendations --all` to see the full status."
        );
    }

    let mut lines = vec!["LSP Plugin Recommendations".to_string()];
    lines.push(format!(
        "Language: {}",
        language.unwrap_or("all (project-detected)")
    ));
    lines.push(String::new());

    // Group by status
    let mut new_recs = Vec::new();
    let mut installed_recs = Vec::new();
    let mut dismissed_recs = Vec::new();

    for rec in &filtered {
        if rec.is_dismissed {
            dismissed_recs.push(rec);
        } else if rec.is_already_installed {
            installed_recs.push(rec);
        } else {
            new_recs.push(rec);
        }
    }

    if !new_recs.is_empty() {
        lines.push("New recommendations:".to_string());
        for rec in &new_recs {
            lines.push(format!(
                "  {:<24} confidence: {:.0}%  {}",
                rec.plugin_name,
                rec.confidence * 100.0,
                rec.description,
            ));
        }
        lines.push(String::new());
    }

    if !installed_recs.is_empty() {
        lines.push("Already installed:".to_string());
        for rec in &installed_recs {
            lines.push(format!(
                "  {:<24} (muted if unwanted via /lsp)",
                rec.plugin_name
            ));
        }
        lines.push(String::new());
    }

    if !dismissed_recs.is_empty() {
        lines.push("Dismissed:".to_string());
        for rec in &dismissed_recs {
            lines.push(format!("  {}", rec.plugin_name));
        }
        lines.push(String::new());
    }

    lines.push("To install: `/plugin install <plugin_id>`".to_string());
    lines.join("\n")
}

fn usage() -> String {
    "Usage:\n  /lsp status\n  /lsp servers\n  /lsp recommendations [--all]\n  /lsp recommend [language]\n  /lsp help".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_bootstrap::SessionId;
    use allthecodes_engine::types::app_state::AppState;
    use std::path::PathBuf;

    fn test_ctx() -> CommandContext {
        CommandContext {
            messages: Vec::new(),
            cwd: PathBuf::from("/test/project"),
            app_state: AppState::default(),
            session_id: SessionId::new(),
        }
    }

    #[tokio::test]
    async fn lsp_status_renders_server_cards() {
        let handler = LspHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("status", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("LSP server status"));
                assert!(
                    text.contains("extensions:") || text.contains("No LSP servers configured.")
                );
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn lsp_recommendations_renders_settings() {
        let handler = LspHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("recommendations", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("LSP recommendation settings"));
                assert!(text.contains("muted plugins"));
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn lsp_recommend_returns_output() {
        let handler = LspHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("recommend", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(!text.is_empty());
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn lsp_recommend_with_language_returns_output() {
        let handler = LspHandler;
        let mut ctx = test_ctx();
        let result = handler.execute("recommend rust", &mut ctx).await.unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(!text.is_empty());
            }
            _ => panic!("expected Output"),
        }
    }

    #[tokio::test]
    async fn lsp_recommendations_all_renders_recommendations() {
        let handler = LspHandler;
        let mut ctx = test_ctx();
        let result = handler
            .execute("recommendations --all", &mut ctx)
            .await
            .unwrap();
        match result {
            CommandResult::Output(text) => {
                assert!(text.contains("LSP recommendation settings"));
            }
            _ => panic!("expected Output"),
        }
    }
}
