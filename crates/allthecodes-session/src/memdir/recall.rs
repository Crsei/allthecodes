use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use super::index::{list_memories, memory_identity, query_requests_memory_ignore};
use super::types::{MemoryEntry, MemoryScope, MemoryType, RelevantMemory};

fn recall_scopes(include_auto: bool) -> Vec<MemoryScope> {
    let mut scopes = vec![MemoryScope::Project, MemoryScope::Global];
    if allthecodes_config::features::enabled(allthecodes_config::features::Feature::TeamMemory) {
        scopes.push(MemoryScope::Team);
    }
    if include_auto {
        scopes.push(MemoryScope::Auto);
    }
    scopes
}

fn tokenize_for_recall(value: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();

    for ch in value.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '-' {
            current.push(ch.to_ascii_lowercase());
        } else if current.len() >= 3 {
            terms.push(current.clone());
            current.clear();
        } else {
            current.clear();
        }
    }

    if current.len() >= 3 {
        terms.push(current);
    }

    terms.sort();
    terms.dedup();
    terms
}

fn entry_recall_text(entry: &MemoryEntry) -> String {
    let mut text = format!("{} {} {}", entry.key, entry.category, entry.value);
    if let Some(memory_type) = entry.effective_memory_type() {
        text.push(' ');
        text.push_str(memory_type.as_str());
    }
    if let Some(description) = &entry.description {
        text.push(' ');
        text.push_str(description);
    }
    for term in &entry.search_terms {
        text.push(' ');
        text.push_str(term);
    }
    text
}

fn score_memory_for_query(entry: &MemoryEntry, query: &str) -> Option<(u32, Vec<String>)> {
    let query_terms = tokenize_for_recall(query);
    if query_terms.is_empty() {
        return None;
    }

    let key = entry.key.to_ascii_lowercase();
    let category = entry.category.to_ascii_lowercase();
    let value = entry.value.to_ascii_lowercase();
    let description = entry
        .description
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let search_terms = entry
        .search_terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let memory_type = entry
        .effective_memory_type()
        .map(|memory_type| memory_type.as_str().to_string())
        .unwrap_or_default();

    let mut score = 0u32;
    let mut matched = Vec::new();
    for term in query_terms {
        let mut term_score = 0u32;
        if key.contains(&term) {
            term_score += 8;
        }
        if category.contains(&term) || memory_type.contains(&term) {
            term_score += 5;
        }
        if description.contains(&term) || search_terms.iter().any(|value| value.contains(&term)) {
            term_score += 4;
        }
        if value.contains(&term) {
            term_score += 2;
        }

        if term_score > 0 {
            score += term_score;
            matched.push(term);
        }
    }

    let query_lower = query.trim().to_ascii_lowercase();
    if query_lower.len() >= 8
        && entry_recall_text(entry)
            .to_ascii_lowercase()
            .contains(&query_lower)
    {
        score += 12;
    }

    (score > 0).then_some((score, matched))
}

fn is_generic_recent_tool_memory(entry: &MemoryEntry, recent_tool_names: &[String]) -> bool {
    if recent_tool_names.is_empty() {
        return false;
    }

    let text = entry_recall_text(entry).to_ascii_lowercase();
    if "avoid|bug|danger|do not|error|failure|issue|known|pitfall|risk|security|warning"
        .split('|')
        .any(|marker| text.contains(marker))
    {
        return false;
    }

    let looks_generic = "docs|example|examples|guide|how to|manual|reference|syntax|tool|usage"
        .split('|')
        .any(|marker| text.contains(marker))
        || entry.effective_memory_type() == Some(MemoryType::Reference);
    if !looks_generic {
        return false;
    }

    recent_tool_names.iter().any(|tool| {
        let tool = tool.trim().to_ascii_lowercase();
        !tool.is_empty() && text.contains(&tool)
    })
}

pub fn recall_relevant_memories(
    cwd: &Path,
    include_auto: bool,
    query: &str,
    recent_tool_names: &[String],
    already_surfaced: &HashSet<String>,
    max_results: usize,
) -> Result<Vec<RelevantMemory>> {
    if max_results == 0 || query.trim().is_empty() || query_requests_memory_ignore(query) {
        return Ok(Vec::new());
    }

    let mut relevant = Vec::new();
    for scope in recall_scopes(include_auto) {
        for entry in list_memories(scope, cwd).unwrap_or_default() {
            let identity = memory_identity(scope, &entry);
            if already_surfaced.contains(&identity)
                || is_generic_recent_tool_memory(&entry, recent_tool_names)
            {
                continue;
            }

            let Some((score, matched_terms)) = score_memory_for_query(&entry, query) else {
                continue;
            };
            relevant.push(RelevantMemory {
                identity,
                scope,
                entry,
                score,
                matched_terms,
            });
        }
    }

    relevant.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.entry.updated_at.cmp(&a.entry.updated_at))
            .then_with(|| a.identity.cmp(&b.identity))
    });
    relevant.truncate(max_results);
    Ok(relevant)
}

pub fn format_relevant_memory_context(relevant: &[RelevantMemory]) -> String {
    let mut sections = Vec::new();
    for scope in recall_scopes(true) {
        let memories = relevant
            .iter()
            .filter(|memory| memory.scope == scope)
            .collect::<Vec<_>>();
        if memories.is_empty() {
            continue;
        }

        let mut section = format!("## {}\n", scope.context_title());
        for memory in memories {
            let matched = if memory.matched_terms.is_empty() {
                String::new()
            } else {
                format!(" matched: {}", memory.matched_terms.join(", "))
            };
            section.push_str(&format!(
                "- **{}**{} ({}; score {}{}): {}\n",
                memory.entry.key,
                memory.entry.bracketed_label(),
                memory.identity,
                memory.score,
                matched,
                memory.entry.value
            ));
        }
        sections.push(section);
    }

    if sections.is_empty() {
        String::new()
    } else {
        format!(
            "<memory-context>\n## Relevant Memories\nUse these recalled memories only when they are relevant to the current request. If the user explicitly asks to ignore memory, do not use memory context.\n{}</memory-context>",
            sections.join("\n")
        )
    }
}

pub fn build_model_assisted_recall_prompt(
    query: &str,
    candidates: &[RelevantMemory],
    max_results: usize,
) -> String {
    let mut prompt = format!(
        "Select up to {max_results} memories that are relevant to the user request.\n\
         Return only a JSON array of memory identity strings, ordered by usefulness.\n\
         Do not invent identities. Return [] if none are relevant.\n\n\
         User request:\n{}\n\n\
         Candidate memories:\n",
        truncate_for_model_recall(query, 1200)
    );

    for memory in candidates {
        prompt.push_str(&format!(
            "- identity: {}\n  key: {}{}\n  scope: {}\n  type: {}\n  score: {}\n  description: {}\n  value: {}\n",
            memory.identity,
            memory.entry.key,
            memory.entry.bracketed_label(),
            memory.scope.as_str(),
            memory
                .entry
                .effective_memory_type()
                .map(|memory_type| memory_type.as_str())
                .unwrap_or(""),
            memory.score,
            memory.entry.description.as_deref().unwrap_or(""),
            truncate_for_model_recall(&memory.entry.value, 500),
        ));
    }

    prompt
}

pub fn parse_model_assisted_recall_selection(
    response: &str,
    candidates: &[RelevantMemory],
    max_results: usize,
) -> Vec<String> {
    let candidate_identities = candidates
        .iter()
        .map(|memory| memory.identity.as_str())
        .collect::<HashSet<_>>();

    let mut selected = Vec::new();
    for identity in parse_identity_candidates_from_response(response) {
        if candidate_identities.contains(identity.as_str()) && !selected.contains(&identity) {
            selected.push(identity);
            if selected.len() >= max_results {
                return selected;
            }
        }
    }

    if !selected.is_empty() {
        return selected;
    }

    for memory in candidates {
        if response.contains(&memory.identity)
            || response.split_whitespace().any(|token| {
                token.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                    == memory.entry.key
            })
        {
            selected.push(memory.identity.clone());
            if selected.len() >= max_results {
                break;
            }
        }
    }
    selected
}

pub fn select_relevant_memories_by_identity(
    candidates: &[RelevantMemory],
    identities: &[String],
    max_results: usize,
) -> Vec<RelevantMemory> {
    let mut selected = Vec::new();
    for identity in identities {
        if let Some(memory) = candidates
            .iter()
            .find(|memory| &memory.identity == identity)
        {
            if !selected
                .iter()
                .any(|selected_memory: &RelevantMemory| selected_memory.identity == memory.identity)
            {
                selected.push(memory.clone());
                if selected.len() >= max_results {
                    break;
                }
            }
        }
    }
    selected
}

fn parse_identity_candidates_from_response(response: &str) -> Vec<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(response.trim()) {
        return identity_candidates_from_json(&value);
    }

    if let (Some(start), Some(end)) = (response.find('['), response.rfind(']')) {
        if start < end {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&response[start..=end]) {
                return identity_candidates_from_json(&value);
            }
        }
    }

    Vec::new()
}

fn identity_candidates_from_json(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect(),
        serde_json::Value::Object(map) => ["selected", "memories", "ids"]
            .iter()
            .filter_map(|key| map.get(*key))
            .flat_map(identity_candidates_from_json)
            .collect(),
        _ => Vec::new(),
    }
}

fn truncate_for_model_recall(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

/// Build query-scoped memory context and return the surfaced identities.
pub fn build_relevant_memory_context_with(
    cwd: &Path,
    include_auto: bool,
    query: &str,
    recent_tool_names: &[String],
    already_surfaced: &HashSet<String>,
    max_results: usize,
) -> Result<(String, Vec<String>)> {
    let relevant = recall_relevant_memories(
        cwd,
        include_auto,
        query,
        recent_tool_names,
        already_surfaced,
        max_results,
    )?;
    let surfaced = relevant
        .iter()
        .map(|memory| memory.identity.clone())
        .collect::<Vec<_>>();
    Ok((format_relevant_memory_context(&relevant), surfaced))
}
