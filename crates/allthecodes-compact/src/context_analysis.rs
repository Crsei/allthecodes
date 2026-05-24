//! Context window analysis — what actually reaches the API after the
//! pre-send pipeline.

use serde::Serialize;

use allthecodes_types::message::Message;
use allthecodes_utils::tokens;

use super::auto_compact;
use super::microcompact;
use super::snip;

const DEFAULT_SNIP_MAX_TURNS: usize = 200;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ContextCategory {
    pub label: String,
    pub tokens: u64,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ContextAnalysis {
    pub model: String,
    pub context_window: u64,
    pub total_used: u64,
    pub total_percent: f32,
    pub compacted: bool,
    pub messages_in: usize,
    pub messages_out: usize,
    pub categories: Vec<ContextCategory>,
    pub unavailable_categories: Vec<String>,
    pub estimation_notes: Vec<String>,
}

#[derive(Debug, Default)]
pub struct ContextAnalysisInput<'a> {
    pub messages: &'a [Message],
    pub system_prompt: Option<&'a str>,
    pub skills_manifest: Option<&'a str>,
    pub cached_files_chars: u64,
    pub tools_schema: Option<&'a str>,
    pub hook_results: Option<&'a str>,
    pub model: &'a str,
}

pub fn analyze_context_usage(input: ContextAnalysisInput<'_>) -> ContextAnalysis {
    analyze_context_usage_with_window(input, None)
}

pub fn analyze_context_usage_with_window(
    input: ContextAnalysisInput<'_>,
    context_window: Option<u64>,
) -> ContextAnalysis {
    let context_window =
        context_window.unwrap_or_else(|| auto_compact::get_context_window_size(input.model));
    let messages_in = input.messages.len();
    let mut compacted = false;
    let snipped = snip::snip_compact_if_needed(input.messages.to_vec(), DEFAULT_SNIP_MAX_TURNS);
    if snipped.tokens_freed > 0 {
        compacted = true;
    }
    let micro = microcompact::microcompact_messages(snipped.messages);
    if micro.tokens_freed > 0 {
        compacted = true;
    }
    let effective = micro.messages;
    let messages_out = effective.len();

    let messages_tokens = tokens::estimate_messages_tokens(&effective);
    let system_tokens = input
        .system_prompt
        .map(tokens::estimate_tokens)
        .unwrap_or(0);
    let skills_tokens = input
        .skills_manifest
        .map(tokens::estimate_tokens)
        .unwrap_or(0);
    let tools_tokens = input.tools_schema.map(tokens::estimate_tokens).unwrap_or(0);
    let hooks_tokens = input.hook_results.map(tokens::estimate_tokens).unwrap_or(0);
    let files_tokens = if input.cached_files_chars == 0 {
        0
    } else {
        ((input.cached_files_chars as f64) / 4.0).ceil() as u64
    };

    let total_used = messages_tokens
        .saturating_add(system_tokens)
        .saturating_add(skills_tokens)
        .saturating_add(tools_tokens)
        .saturating_add(hooks_tokens)
        .saturating_add(files_tokens);
    let capped_used = total_used.min(context_window);
    let free = context_window.saturating_sub(capped_used);

    let mut rows: Vec<ContextCategory> = vec![
        row("messages", messages_tokens, context_window),
        row("cached input", files_tokens, context_window),
        row("hook results", hooks_tokens, context_window),
    ];
    let mut unavailable_categories = Vec::new();
    if input.system_prompt.is_some() {
        rows.push(row("system prompt", system_tokens, context_window));
    } else {
        unavailable_categories.push("system prompt".to_string());
    }
    if input.skills_manifest.is_some() {
        rows.push(row("skills", skills_tokens, context_window));
    }
    if input.tools_schema.is_some() {
        rows.push(row("tools schema", tools_tokens, context_window));
    } else {
        unavailable_categories.push("tools schema".to_string());
    }
    rows.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    rows.push(row("free", free, context_window));

    let total_percent = percent_of(capped_used, context_window);

    ContextAnalysis {
        model: input.model.to_string(),
        context_window,
        total_used,
        total_percent,
        compacted,
        messages_in,
        messages_out,
        categories: rows,
        unavailable_categories,
        estimation_notes: vec![
            "Token counts are estimated with the standard ~4-chars/token heuristic.".to_string(),
            "This is an estimated conversation context view, not an exact provider API accounting."
                .to_string(),
            "System prompt and tools schema are only shown when the command context provides real values."
                .to_string(),
        ],
    }
}

fn row(label: &str, tokens: u64, window: u64) -> ContextCategory {
    ContextCategory {
        label: label.to_string(),
        tokens,
        percent: percent_of(tokens, window),
    }
}

fn percent_of(n: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    ((n as f64 / total as f64) * 100.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{AssistantMessage, ContentBlock, MessageContent, UserMessage};
    use uuid::Uuid;

    fn make_user(text: &str) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".into(),
            content: MessageContent::Text(text.into()),
            is_meta: false,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    fn make_assistant(text: &str) -> Message {
        Message::Assistant(AssistantMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "assistant".into(),
            content: vec![ContentBlock::Text { text: text.into() }],
            usage: None,
            stop_reason: Some("end_turn".into()),
            is_api_error_message: false,
            api_error: None,
            cost_usd: 0.0,
        })
    }

    #[test]
    fn test_empty_inputs_yield_zero_used() {
        let report = analyze_context_usage(ContextAnalysisInput {
            messages: &[],
            model: "claude-sonnet-4-20250514",
            ..Default::default()
        });
        assert_eq!(report.total_used, 0);
        assert_eq!(report.total_percent, 0.0);
        assert_eq!(report.context_window, 200_000);
        assert_eq!(report.categories.len(), 4);
        assert!(report
            .unavailable_categories
            .contains(&"system prompt".to_string()));
        assert!(report
            .unavailable_categories
            .contains(&"tools schema".to_string()));
        let free = report.categories.last().unwrap();
        assert_eq!(free.label, "free");
        assert_eq!(free.tokens, 200_000);
    }

    #[test]
    fn test_non_empty_categories_sum_and_percentages() {
        let messages = vec![make_user("hello world"), make_assistant("hi!")];
        let report = analyze_context_usage(ContextAnalysisInput {
            messages: &messages,
            system_prompt: Some("You are a helpful assistant."),
            skills_manifest: Some("skill a\nskill b"),
            cached_files_chars: 400,
            tools_schema: Some("bash, edit, grep"),
            hook_results: None,
            model: "claude-sonnet-4-20250514",
        });
        let labels: Vec<&str> = report.categories.iter().map(|c| c.label.as_str()).collect();
        for expected in [
            "messages",
            "system prompt",
            "skills",
            "cached input",
            "tools schema",
            "hook results",
            "free",
        ] {
            assert!(labels.contains(&expected));
        }
        let files = report
            .categories
            .iter()
            .find(|c| c.label == "cached input")
            .unwrap();
        assert_eq!(files.tokens, 100);
        let non_free: u64 = report
            .categories
            .iter()
            .filter(|c| c.label != "free")
            .map(|c| c.tokens)
            .sum();
        assert_eq!(non_free, report.total_used);
        let free_tokens = report
            .categories
            .iter()
            .find(|c| c.label == "free")
            .unwrap()
            .tokens;
        assert!(
            report.total_used.min(report.context_window) + free_tokens <= report.context_window
        );
    }

    #[test]
    fn test_percentages_never_exceed_100() {
        let huge_text = "a".repeat(1_000_000);
        let messages = vec![make_user(&huge_text)];
        let report = analyze_context_usage(ContextAnalysisInput {
            messages: &messages,
            model: "claude-sonnet-4-20250514",
            ..Default::default()
        });
        assert!(report.total_used > report.context_window);
        assert!(report.total_percent <= 100.0);
        let free = report
            .categories
            .iter()
            .find(|c| c.label == "free")
            .unwrap();
        assert_eq!(free.tokens, 0);
    }

    #[test]
    fn test_categories_sorted_desc_with_free_last() {
        let messages = vec![make_user("hello world"), make_assistant("hi!")];
        let report = analyze_context_usage(ContextAnalysisInput {
            messages: &messages,
            system_prompt: Some(&"a".repeat(4_000)),
            tools_schema: Some("x"),
            model: "claude-sonnet-4-20250514",
            ..Default::default()
        });
        assert_eq!(report.categories.last().unwrap().label, "free");
        let non_free: Vec<&ContextCategory> = report
            .categories
            .iter()
            .filter(|c| c.label != "free")
            .collect();
        for pair in non_free.windows(2) {
            assert!(pair[0].tokens >= pair[1].tokens);
        }
    }

    #[test]
    fn test_transform_is_applied_for_large_conversations() {
        let mut msgs = Vec::new();
        for i in 0..250 {
            msgs.push(make_user(&format!("user turn {}", i)));
            msgs.push(make_assistant(&format!("assistant turn {}", i)));
        }
        let report = analyze_context_usage(ContextAnalysisInput {
            messages: &msgs,
            model: "claude-sonnet-4-20250514",
            ..Default::default()
        });
        assert!(report.compacted);
        assert!(report.messages_out < report.messages_in);
    }

    #[test]
    fn test_serialize_json_shape() {
        let report = analyze_context_usage(ContextAnalysisInput {
            messages: &[],
            model: "claude-sonnet-4-20250514",
            ..Default::default()
        });
        let json = serde_json::to_value(&report).unwrap();
        for k in [
            "model",
            "context_window",
            "total_used",
            "total_percent",
            "compacted",
            "messages_in",
            "messages_out",
            "unavailable_categories",
            "estimation_notes",
        ] {
            assert!(json.get(k).is_some());
        }
        let cats = json.get("categories").unwrap().as_array().unwrap();
        assert!(!cats.is_empty());
        for c in cats {
            assert!(c.get("label").is_some());
            assert!(c.get("tokens").is_some());
            assert!(c.get("percent").is_some());
        }
    }
}
