use allthecodes_types::message::{ContentBlock, Message, MessageContent, ToolResultContent};

const CONTEXT_WINDOW_ENV: &str = "CLAUDE_CODE_MAX_CONTEXT_TOKENS";

/// Get the context window size for a given model.
/// Returns token count for the model's context window.
pub fn get_context_window_size(model: &str) -> u64 {
    let env_override = std::env::var(CONTEXT_WINDOW_ENV).ok();
    resolve_context_window_size(model, env_override.as_deref())
}

pub fn get_context_window_size_with_settings(
    model: &str,
    settings: Option<&allthecodes_config::runtime_settings::SettingsJson>,
) -> u64 {
    let env_override = std::env::var(CONTEXT_WINDOW_ENV).ok();
    if let Some(value) = env_override
        .as_deref()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
    {
        return value;
    }

    settings
        .and_then(|settings| settings.model_capabilities.get(model))
        .and_then(|capability| capability.context_window)
        .filter(|value| *value > 0)
        .unwrap_or_else(|| resolve_context_window_size(model, None))
}

fn resolve_context_window_size(model: &str, env_override: Option<&str>) -> u64 {
    if let Some(value) = env_override
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
    {
        return value;
    }

    if model.to_ascii_lowercase().contains("[1m]") {
        allthecodes_config::constants::tokens::CONTEXT_WINDOW_1M
    } else {
        allthecodes_config::constants::tokens::MODEL_CONTEXT_WINDOW_DEFAULT
    }
}

/// Average characters per token for English text.
/// This is a rough approximation; actual tokenization varies by model
/// and content type. Code tends to be ~3.5 chars/token, prose ~4-5.
const CHARS_PER_TOKEN: f64 = 4.0;

/// Overhead tokens per message (role metadata, formatting, etc.).
const MESSAGE_OVERHEAD_TOKENS: u64 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenCountMethod {
    Heuristic,
    ProviderExact,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TokenUsageReport {
    pub estimated_tokens: u64,
    pub context_window: u64,
    pub threshold_tokens: u64,
    pub remaining_until_threshold: u64,
    pub utilization_ratio: f64,
    pub over_threshold: bool,
    pub count_method: TokenCountMethod,
    pub exact_count_available: bool,
    pub provider: Option<String>,
}

/// Estimate token count for a string using the ~4 chars per token heuristic.
pub fn estimate_tokens(text: &str) -> u64 {
    if text.is_empty() {
        return 0;
    }
    (text.len() as f64 / CHARS_PER_TOKEN).ceil() as u64
}

/// Estimate token count for a slice of messages.
/// Accounts for per-message overhead and content within each message.
pub fn estimate_messages_tokens(messages: &[Message]) -> u64 {
    messages.iter().map(estimate_message_tokens).sum()
}

/// Estimate token count for a single message.
fn estimate_message_tokens(msg: &Message) -> u64 {
    let content_tokens = match msg {
        Message::User(user) => estimate_message_content_tokens(&user.content),
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .map(estimate_content_block_tokens)
            .sum(),
        Message::System(sys) => estimate_tokens(&sys.content),
        Message::Progress(prog) => estimate_tokens(&prog.data.to_string()),
        Message::Attachment(_) => 50, // rough fixed estimate for attachment metadata
    };

    content_tokens + MESSAGE_OVERHEAD_TOKENS
}

/// Estimate tokens for MessageContent (text or blocks).
fn estimate_message_content_tokens(content: &MessageContent) -> u64 {
    match content {
        MessageContent::Text(text) => estimate_tokens(text),
        MessageContent::Blocks(blocks) => blocks.iter().map(estimate_content_block_tokens).sum(),
    }
}

/// Estimate tokens for a single content block.
fn estimate_content_block_tokens(block: &ContentBlock) -> u64 {
    match block {
        ContentBlock::Text { text } => estimate_tokens(text),
        ContentBlock::ToolUse { id, name, input }
        | ContentBlock::ServerToolUse { id, name, input } => {
            // Tool use has metadata overhead plus the JSON input
            let input_str = input.to_string();
            estimate_tokens(id) + estimate_tokens(name) + estimate_tokens(&input_str) + 10
            // structural overhead
        }
        ContentBlock::ToolResult { content, .. } => estimate_tool_result_content_tokens(content),
        ContentBlock::Thinking { thinking, .. } => estimate_tokens(thinking),
        ContentBlock::RedactedThinking { data } => estimate_tokens(data),
        ContentBlock::ConnectorText { connector_text, .. } => estimate_tokens(connector_text),
        ContentBlock::Image { source } => {
            // Images are typically encoded as base64; the API counts them
            // differently. A rough estimate based on the encoded data size.
            // Most images use ~1600 tokens for a typical screenshot.
            let data_len = source.data.len();
            // Base64 encodes ~3 bytes into 4 chars, so raw size is ~75% of data_len.
            // Then ~750 bytes per token for images is a rough Anthropic estimate.
            let raw_bytes = (data_len as f64 * 0.75) as u64;
            (raw_bytes / 750).max(100)
        }
    }
}

/// Estimate tokens for tool result content.
fn estimate_tool_result_content_tokens(content: &ToolResultContent) -> u64 {
    match content {
        ToolResultContent::Text(text) => estimate_tokens(text),
        ToolResultContent::Blocks(blocks) => blocks.iter().map(estimate_content_block_tokens).sum(),
    }
}

/// Check if estimated tokens in the given messages exceed the model's
/// context window threshold (80% of context window).
///
/// This is used to determine if compaction should be triggered.
pub fn is_over_token_limit(messages: &[Message], model: &str) -> bool {
    estimate_context_usage(messages, model).over_threshold
}

/// Return a structured context-usage report for the current heuristic estimator.
///
/// This deliberately marks `exact_count_available=false` so callers do not
/// mistake allthecodes' hot-path estimate for a provider-level countTokens result.
pub fn estimate_context_usage(messages: &[Message], model: &str) -> TokenUsageReport {
    let estimated_tokens = estimate_messages_tokens(messages);
    token_usage_report_from_count(
        estimated_tokens,
        model,
        TokenCountMethod::Heuristic,
        None::<String>,
    )
}

/// Build a structured usage report from an already-computed token count.
///
/// Provider-backed callers use this after an exact count API returns. The
/// existing heuristic path also funnels through here so threshold math stays
/// identical across methods.
pub fn token_usage_report_from_count(
    token_count: u64,
    model: &str,
    count_method: TokenCountMethod,
    provider: Option<impl Into<String>>,
) -> TokenUsageReport {
    let context_window = get_context_window_size(model);
    let threshold_tokens = (context_window as f64 * 0.8) as u64;
    let remaining_until_threshold = threshold_tokens.saturating_sub(token_count);
    let utilization_ratio = if context_window == 0 {
        0.0
    } else {
        token_count as f64 / context_window as f64
    };
    let exact_count_available = count_method == TokenCountMethod::ProviderExact;

    TokenUsageReport {
        estimated_tokens: token_count,
        context_window,
        threshold_tokens,
        remaining_until_threshold,
        utilization_ratio,
        over_threshold: token_count > threshold_tokens,
        count_method,
        exact_count_available,
        provider: provider.map(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_types::message::{Message, MessageContent, UserMessage};
    use uuid::Uuid;

    fn make_user_message(text: &str, is_meta: bool) -> Message {
        Message::User(UserMessage {
            uuid: Uuid::new_v4(),
            timestamp: 0,
            role: "user".to_string(),
            content: MessageContent::Text(text.to_string()),
            is_meta,
            tool_use_result: None,
            source_tool_assistant_uuid: None,
        })
    }

    #[test]
    fn test_estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn test_estimate_tokens_short() {
        // "hello" = 5 chars, ceil(5/4) = 2 tokens
        assert_eq!(estimate_tokens("hello"), 2);
    }

    #[test]
    fn test_estimate_tokens_longer() {
        // 100 chars -> 25 tokens
        let text = "a".repeat(100);
        assert_eq!(estimate_tokens(&text), 25);
    }

    #[test]
    fn test_estimate_messages_tokens() {
        let messages = vec![
            make_user_message("hello world", false),
            make_user_message("how are you?", false),
        ];

        let tokens = estimate_messages_tokens(&messages);
        // Each message: ceil(len/4) + 4 overhead
        // "hello world" = 11 chars -> ceil(11/4) = 3 + 4 = 7
        // "how are you?" = 12 chars -> ceil(12/4) = 3 + 4 = 7
        // Total = 14
        assert_eq!(tokens, 14);
    }

    #[test]
    fn test_is_over_token_limit() {
        // Threshold for any model is 80% of 200k = 160k tokens
        // A message with enough text to exceed 160k tokens:
        // 160001 tokens * 4 chars = 640004 chars
        let large_text = "a".repeat(640_004);
        let messages = vec![make_user_message(&large_text, false)];
        assert!(is_over_token_limit(&messages, "claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_context_window_supports_1m_suffix() {
        assert_eq!(
            resolve_context_window_size("claude-sonnet-4-20250514[1m]", None),
            1_000_000
        );
    }

    #[test]
    fn test_context_window_env_override_wins() {
        assert_eq!(
            resolve_context_window_size("claude-sonnet-4-20250514[1m]", Some("320000")),
            320_000
        );
    }

    #[test]
    fn test_context_window_invalid_env_falls_back() {
        assert_eq!(
            resolve_context_window_size("claude-sonnet-4-20250514", Some("not-a-number")),
            200_000
        );
        assert_eq!(
            resolve_context_window_size("claude-sonnet-4-20250514[1m]", Some("0")),
            1_000_000
        );
    }

    #[test]
    fn test_context_window_uses_settings_capability() {
        let mut settings = allthecodes_config::runtime_settings::SettingsJson::default();
        settings.model_capabilities.insert(
            "gpt-5.3-codex-spark".to_string(),
            allthecodes_config::settings::ModelCapabilitySettings {
                context_window: Some(128_000),
                ..Default::default()
            },
        );

        assert_eq!(
            get_context_window_size_with_settings("gpt-5.3-codex-spark", Some(&settings)),
            128_000
        );
        assert_eq!(
            get_context_window_size_with_settings("unknown-model", Some(&settings)),
            200_000
        );
    }

    #[test]
    fn test_not_over_token_limit() {
        let messages = vec![make_user_message("hello", false)];
        assert!(!is_over_token_limit(&messages, "claude-sonnet-4-20250514"));
    }

    #[test]
    fn test_is_over_token_limit_respects_1m_suffix() {
        let large_text = "a".repeat(640_004);
        let messages = vec![make_user_message(&large_text, false)];
        assert!(!is_over_token_limit(
            &messages,
            "claude-sonnet-4-20250514[1m]"
        ));
    }

    #[test]
    fn test_estimate_context_usage_reports_threshold_and_method() {
        let messages = vec![make_user_message(&"a".repeat(400), false)];
        let report = estimate_context_usage(&messages, "claude-sonnet-4-20250514");

        assert_eq!(report.estimated_tokens, 104);
        assert_eq!(report.context_window, 200_000);
        assert_eq!(report.threshold_tokens, 160_000);
        assert_eq!(report.remaining_until_threshold, 159_896);
        assert!((report.utilization_ratio - 0.00052).abs() < f64::EPSILON);
        assert!(!report.over_threshold);
        assert_eq!(report.count_method, TokenCountMethod::Heuristic);
        assert!(!report.exact_count_available);
        assert_eq!(report.provider, None);
    }

    #[test]
    fn test_token_usage_report_from_provider_count_marks_exact() {
        let report = token_usage_report_from_count(
            160_001,
            "claude-sonnet-4-20250514",
            TokenCountMethod::ProviderExact,
            Some("anthropic"),
        );

        assert_eq!(report.estimated_tokens, 160_001);
        assert_eq!(report.context_window, 200_000);
        assert_eq!(report.threshold_tokens, 160_000);
        assert_eq!(report.remaining_until_threshold, 0);
        assert!(report.over_threshold);
        assert_eq!(report.count_method, TokenCountMethod::ProviderExact);
        assert!(report.exact_count_available);
        assert_eq!(report.provider.as_deref(), Some("anthropic"));
    }
}
