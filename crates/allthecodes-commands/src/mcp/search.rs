use anyhow::Result;
use std::path::Path;

use crate::{search_format::format_discovery_search_results, CommandResult};

pub(super) fn handle_search(args: &str, workspace: &Path) -> Result<CommandResult> {
    let query = args
        .trim()
        .strip_prefix("search")
        .map(str::trim)
        .unwrap_or_default();
    if query.is_empty() {
        return Ok(CommandResult::Output(
            "Usage: /mcp search <query>".to_string(),
        ));
    }

    let context = match allthecodes_tools::discovery_search::DiscoveryContext::new(workspace) {
        Ok(context) => context,
        Err(error) => {
            return Ok(CommandResult::Output(format!("MCP search failed: {error}")));
        }
    };
    match allthecodes_tools::discovery_search::run_mcp_search_with_context(
        allthecodes_tools::discovery_search::DiscoverySearchInput {
            query: query.to_string(),
            source_filter: None,
            max_results: 10,
            include_summaries: true,
        },
        &context,
    ) {
        Ok(output) => {
            let mut notes = vec![
                "Use /mcp status for connection health and server status.",
                "Use ToolSearch source=mcp for exact callable tool schemas.",
            ];
            if let Some(preview) = output.display_preview.as_deref() {
                notes.push(preview);
            }
            let results = serde_json::from_value::<
                Vec<allthecodes_tools::discovery_search::DiscoverySearchResult>,
            >(output.data["results"].clone())
            .unwrap_or_default();
            Ok(CommandResult::Output(format_discovery_search_results(
                "MCP search results",
                query,
                &results,
                &notes,
            )))
        }
        Err(error) => Ok(CommandResult::Output(format!("MCP search failed: {error}"))),
    }
}
