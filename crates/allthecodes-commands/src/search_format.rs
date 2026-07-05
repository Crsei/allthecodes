use allthecodes_tools::discovery_search::{DiscoveryResultKind, DiscoverySearchResult};

pub(crate) fn format_discovery_search_results(
    title: &str,
    query: &str,
    results: &[DiscoverySearchResult],
    extra_notes: &[&str],
) -> String {
    let mut lines = Vec::new();
    if results.is_empty() {
        lines.push(format!("No {title} for '{query}'."));
    } else {
        lines.push(format!("{title} for '{query}' ({}):", results.len()));
        lines.push(String::new());
        for (index, result) in results.iter().enumerate() {
            lines.push(format!(
                "  {}. {} [{}]{}{}",
                index + 1,
                result_label(result),
                kind_label(result.kind),
                result
                    .source
                    .as_deref()
                    .map(|source| format!(" source={source}"))
                    .unwrap_or_default(),
                result
                    .status_summary
                    .as_ref()
                    .map(|status| format!(" status={}", status.state))
                    .unwrap_or_default()
            ));
            if let Some(description) = result.description.as_deref() {
                if !description.trim().is_empty() {
                    lines.push(format!("     {}", truncate(description, 96)));
                }
            }
            if !result.match_reasons.is_empty() {
                lines.push(format!("     matches: {}", result.match_reasons.join(", ")));
            }
            if let Some(action) = result.next_action.as_ref() {
                lines.push(format!("     Next: {} -> {}", action.label, action.command));
            }
        }
    }

    if !extra_notes.is_empty() {
        lines.push(String::new());
        for note in extra_notes {
            lines.push(format!("Hint: {note}"));
        }
    }
    lines.join("\n")
}

fn result_label(result: &DiscoverySearchResult) -> String {
    result
        .display_name
        .as_deref()
        .or(result.id.as_deref())
        .unwrap_or(&result.name)
        .to_string()
}

fn kind_label(kind: DiscoveryResultKind) -> &'static str {
    match kind {
        DiscoveryResultKind::Skill => "skill",
        DiscoveryResultKind::McpServer => "mcp-server",
        DiscoveryResultKind::McpResource => "mcp-resource",
        DiscoveryResultKind::McpCapability => "mcp-capability",
        DiscoveryResultKind::McpSkill => "mcp-skill",
        DiscoveryResultKind::Plugin => "plugin",
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}...", &value[..max.saturating_sub(3)])
    }
}
