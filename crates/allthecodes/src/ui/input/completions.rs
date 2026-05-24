//! Unified completion model for all completion types.
//!
//! Provides `CompletionItem`, `CompletionKind`, `CompletionContext`, and the
//! `CompletionProvider` trait that every completion source implements.
//!
//! Also includes the `CommandCompletionProvider` which wraps Lane C's
//! `DynamicRegistry` for slash-command completions.

use std::fmt;
use std::ops::Range;

use allthecodes_commands::dynamic_registry::CommandSource;
use allthecodes_commands::DYNAMIC_REGISTRY;

use crate::ui::fuzzy_match::best_fuzzy_match;

// ---------------------------------------------------------------------------
// CompletionKind
// ---------------------------------------------------------------------------

/// The kind of completion item — determines rendering and insertion behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompletionKind {
    /// Slash command (`/help`, `/config`, ...)
    Command,
    /// File-system path (`src/main.rs`, `~/docs/`, ...)
    Path,
    /// Shell history prefix (`!git`, `!cargo`, ...)
    ShellHistory,
    /// Slack channel name (`#general`, `#project-alpha`, ...)
    SlackChannel,
}

impl CompletionKind {
    pub fn label(&self) -> &'static str {
        match self {
            CompletionKind::Command => "cmd",
            CompletionKind::Path => "path",
            CompletionKind::ShellHistory => "hist",
            CompletionKind::SlackChannel => "slack",
        }
    }
}

// ---------------------------------------------------------------------------
// CompletionItem
// ---------------------------------------------------------------------------

/// A single candidate in the completion popup.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    /// The kind of completion (determines icon / grouping).
    pub kind: CompletionKind,
    /// Display label shown in the popup (e.g. `/help` or `src/main.rs`).
    pub label: String,
    /// Text used for fuzzy filtering (usually lowercased, no prefix).
    pub filter_text: String,
    /// Text to insert when this item is selected.
    pub insert_text: String,
    /// Optional detail line shown next to or below the label.
    pub detail: Option<String>,
    /// The range within `CompletionContext.input` that this item replaces.
    /// `start..end` are byte offsets.
    pub range: Range<usize>,
    /// Optional ghost suffix text shown dimmed after the cursor.
    pub ghost_suffix: Option<String>,
    /// Source for grouping in the UI (e.g. "Builtin", "Plugin", "Skill").
    pub source_group: Option<&'static str>,
}

impl CompletionItem {
    /// Create a simple completion item with the given label and insert text.
    pub fn new(
        kind: CompletionKind,
        label: impl Into<String>,
        insert_text: impl Into<String>,
        range: Range<usize>,
    ) -> Self {
        let label = label.into();
        let insert_text = insert_text.into();
        let filter_text = label.to_ascii_lowercase();
        Self {
            kind,
            label,
            filter_text,
            insert_text,
            detail: None,
            range,
            ghost_suffix: None,
            source_group: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_filter_text(mut self, filter_text: impl Into<String>) -> Self {
        self.filter_text = filter_text.into();
        self
    }

    pub fn with_ghost_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.ghost_suffix = Some(suffix.into());
        self
    }

    pub fn with_source_group(mut self, group: &'static str) -> Self {
        self.source_group = Some(group);
        self
    }
}

// ---------------------------------------------------------------------------
// CompletionContext
// ---------------------------------------------------------------------------

/// Context supplied to every completion provider when computing candidates.
pub struct CompletionContext<'a> {
    /// The full current input text.
    pub input: &'a str,
    /// Byte offset of the cursor within `input`.
    pub cursor_pos: usize,
}

impl<'a> CompletionContext<'a> {
    pub fn new(input: &'a str, cursor_pos: usize, _skill_scores: &'a [(String, f64)]) -> Self {
        Self { input, cursor_pos }
    }
}

// ---------------------------------------------------------------------------
// CompletionProvider trait
// ---------------------------------------------------------------------------

/// A source of completion items.
///
/// Each provider is registered with a name and priority. Higher-priority
/// providers are consulted first, and their results are shown before
/// lower-priority ones.
pub trait CompletionProvider: Send + Sync {
    /// Compute completion items for the given context.
    ///
    /// This is called synchronously and should return quickly. Providers that
    /// need I/O (file scanning, MCP calls) should cache results.
    fn compute(&self, ctx: &CompletionContext) -> Vec<CompletionItem>;

    /// Priority: higher values mean items appear first in the combined list.
    /// Typical ranges:
    ///   100+  — command completions
    ///    80   — path completions
    ///    60   — shell history completions
    ///    40   — slack channel completions
    fn priority(&self) -> u32;
}

// ---------------------------------------------------------------------------
// CommandCompletionProvider
// ---------------------------------------------------------------------------

/// Completion provider for slash commands.
///
/// Wraps Lane C's `DynamicRegistry` (and the builtin `get_all_commands()` list)
/// to produce command completion items. Supports:
///
/// - Slash-only triggers (`/` at start of input)
/// - Mid-input slash detection (`some text /cmd` mid-line)
/// - Hidden command exact-name priority
/// - Empty-query grouping by source with usage ranking
/// - Ghost suffix for partial commands
pub struct CommandCompletionProvider;

impl CommandCompletionProvider {
    pub fn new() -> Self {
        Self
    }

    /// Find a `/cmd` token at or before the cursor.
    fn find_slash_token(input: &str, cursor_pos: usize) -> Option<(Range<usize>, String)> {
        let byte_pos = cursor_pos.min(input.len());
        let prefix = &input[..byte_pos];

        // Find the last `/` that starts a potential command token
        if let Some(slash_pos) = prefix.rfind('/') {
            // The token must start right after a word boundary (whitespace or BOL)
            if slash_pos == 0 || input.as_bytes()[slash_pos - 1] == b' ' {
                let after_slash = &input[slash_pos + 1..byte_pos];
                // Only alphanumeric, hyphens, underscores — no spaces
                if !after_slash.contains(char::is_whitespace) {
                    let end = byte_pos;
                    return Some((slash_pos..end, after_slash.to_string()));
                }
            }
        }

        None
    }
}

impl CompletionProvider for CommandCompletionProvider {
    fn compute(&self, ctx: &CompletionContext) -> Vec<CompletionItem> {
        let input = ctx.input;
        let cursor_pos = ctx.cursor_pos;

        // Step 1: Detect slash-qualified token at cursor
        let (range, query) = match Self::find_slash_token(input, cursor_pos) {
            Some((r, q)) => (r, q),
            None => return Vec::new(),
        };

        let query_lower = query.to_ascii_lowercase();

        // Step 2: Gather all commands from dynamic registry + builtins
        let registry = DYNAMIC_REGISTRY.lock();
        let all_entries = registry.list_all();

        // Build a map of command-name -> usage_score from the registry
        let usage_map: std::collections::HashMap<&str, f64> = all_entries
            .iter()
            .map(|e| (e.name.as_str(), e.usage_score))
            .collect();
        let source_priority_map: std::collections::HashMap<&str, u8> = all_entries
            .iter()
            .map(|e| (e.name.as_str(), e.source.priority()))
            .collect();

        // Also get builtin commands via allthecodes_commands
        let builtins = allthecodes_commands::get_all_commands();

        // Step 3: Collect matching items
        let mut items: Vec<CompletionItem> = Vec::new();

        // Check builtins first
        for cmd in &builtins {
            let name = &cmd.name;
            let aliases = &cmd.aliases;
            let description = &cmd.description;

            let all_names =
                std::iter::once(name.as_str()).chain(aliases.iter().map(String::as_str));

            let _matched = match best_fuzzy_match(
                all_names
                    .clone()
                    .chain(std::iter::once(description.as_str())),
                &query_lower,
            ) {
                Some(m) => m,
                None => continue,
            };

            let insert = format!("/{} ", name);
            let label = format!("/{}", name);

            let _usage = usage_map.get(name.as_str()).copied().unwrap_or(0.0);
            let source_group = "Builtin";

            let typed_len = query.len().min(name.len());
            let ghost_suffix = if query_lower.is_empty() {
                None
            } else {
                Some(name[typed_len..].to_string())
            };

            let mut item =
                CompletionItem::new(CompletionKind::Command, &label, &insert, range.clone())
                    .with_filter_text(name.to_ascii_lowercase())
                    .with_detail(description.clone())
                    .with_source_group(source_group);

            item.ghost_suffix = ghost_suffix;

            items.push(item);
        }

        // Then check dynamic registry entries not already covered by builtins
        for entry in &*all_entries {
            let name = &entry.name;
            let description = &entry.description;

            // Skip if already matched by builtins
            if builtins.iter().any(|b| b.name == *name) {
                continue;
            }

            let all_candidates = std::iter::once(name.as_str())
                .chain(entry.aliases.iter().map(String::as_str))
                .chain(std::iter::once(description.as_str()));

            let _matched = match best_fuzzy_match(all_candidates, &query_lower) {
                Some(m) => m,
                None => continue,
            };

            let source_group = match entry.source {
                CommandSource::Builtin => "Builtin",
                CommandSource::User => "User",
                CommandSource::Project => "Project",
                CommandSource::Plugin => "Plugin",
                CommandSource::Skill => "Skill",
            };

            let insert = format!("/{} ", name);
            let label = format!("/{}", name);
            let typed_len = query.len().min(name.len());

            let mut item =
                CompletionItem::new(CompletionKind::Command, &label, &insert, range.clone())
                    .with_filter_text(name.to_ascii_lowercase())
                    .with_detail(description.clone())
                    .with_source_group(source_group);
            if !query_lower.is_empty() {
                item.ghost_suffix = Some(name[typed_len..].to_string());
            }
            items.push(item);
        }

        // Step 4: Sort — exact matches first, then by source priority + usage
        items.sort_by(|a, b| {
            let a_is_exact = a.filter_text == query_lower;
            let b_is_exact = b.filter_text == query_lower;
            let a_name = a.label.trim_start_matches('/');
            let b_name = b.label.trim_start_matches('/');
            let a_priority = if builtins.iter().any(|cmd| cmd.name == a_name) {
                CommandSource::Builtin.priority()
            } else {
                source_priority_map.get(a_name).copied().unwrap_or_default()
            };
            let b_priority = if builtins.iter().any(|cmd| cmd.name == b_name) {
                CommandSource::Builtin.priority()
            } else {
                source_priority_map.get(b_name).copied().unwrap_or_default()
            };
            b_is_exact
                .cmp(&a_is_exact)
                .then_with(|| b_priority.cmp(&a_priority))
                .then_with(|| {
                    let a_usage = usage_map.get(a_name).copied().unwrap_or(0.0);
                    let b_usage = usage_map.get(b_name).copied().unwrap_or(0.0);
                    b_usage
                        .partial_cmp(&a_usage)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });

        items
    }

    fn priority(&self) -> u32 {
        100
    }
}

impl Default for CommandCompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CombinedCompleter
// ---------------------------------------------------------------------------

/// Aggregates all registered `CompletionProvider`s into a single completion
/// session. Manages the active list of items and the current selection index.
pub struct CombinedCompleter {
    providers: Vec<Box<dyn CompletionProvider>>,
}

impl CombinedCompleter {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Register a completion provider. Providers with higher priority should
    /// be added first, but `compute()` sorts by priority automatically.
    pub fn add_provider(&mut self, provider: Box<dyn CompletionProvider>) {
        self.providers.push(provider);
    }

    /// Compute all completion items from all providers, sorted by priority
    /// and then by label.
    pub fn compute(&self, ctx: &CompletionContext) -> Vec<CompletionItem> {
        let mut all: Vec<(u32, usize, CompletionItem)> = Vec::new();

        for provider in &self.providers {
            let priority = provider.priority();
            for item in provider.compute(ctx) {
                let seq = all.len();
                all.push((priority, seq, item));
            }
        }

        // Preserve provider-internal ordering inside the same priority band.
        all.sort_by(|(pa, seq_a, _), (pb, seq_b, _)| pb.cmp(pa).then_with(|| seq_a.cmp(seq_b)));

        all.into_iter().map(|(_, _, item)| item).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn provider_count(&self) -> usize {
        self.providers.len()
    }
}

impl Default for CombinedCompleter {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CombinedCompleter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CombinedCompleter")
            .field("provider_count", &self.providers.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Helper: find the command token range in input
// ---------------------------------------------------------------------------

/// Find the range of a `/command` token that the cursor is at or after.
/// Returns `None` if no slash token is found.
pub fn find_command_token_range(input: &str, cursor_pos: usize) -> Option<Range<usize>> {
    let byte_pos = cursor_pos.min(input.len());
    let prefix = &input[..byte_pos];

    // Find the last `/` preceded by whitespace or at BOL
    let slash_pos = prefix.rfind('/')?;
    if slash_pos > 0
        && input
            .as_bytes()
            .get(slash_pos - 1)
            .map(|&b| b != b' ')
            .unwrap_or(false)
    {
        return None; // `/` is part of a word, not a command
    }

    // Extract the partial command after `/`
    let after_slash = &input[slash_pos + 1..byte_pos];
    if after_slash.contains(char::is_whitespace) {
        return None; // Command already has arguments — no completion
    }

    Some(slash_pos..byte_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_context(input: &str, cursor_pos: usize) -> CompletionContext<'_> {
        CompletionContext::new(input, cursor_pos, &[])
    }

    #[test]
    fn command_completion_slash_only() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("/", 1);
        let items = provider.compute(&ctx);
        // Should return many items when only `/` is typed
        assert!(!items.is_empty(), "expected completions for bare slash");
        assert!(items.iter().all(|i| i.label.starts_with('/')));
    }

    #[test]
    fn command_completion_mid_input() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("look at /help for", 10);
        let items = provider.compute(&ctx);
        // "/help" is after "look at " so cursor_pos 10 points at '/help f...'
        // The token is at byte 8 ("/help"), so cursor 10 is inside the word
        assert!(
            items.iter().any(|i| i.label == "/help"),
            "expected /help in mid-input completions"
        );
    }

    #[test]
    fn command_completion_hidden_exact_priority() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("/exit", 5);
        let items = provider.compute(&ctx);
        let exact = items.iter().find(|i| i.label == "/exit");
        assert!(
            exact.is_some(),
            "exact match for /exit should appear even if hidden"
        );
    }

    #[test]
    fn empty_query_shows_items() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("/", 1);
        let items = provider.compute(&ctx);
        assert!(!items.is_empty(), "empty query should return all commands");
    }

    #[test]
    fn ghost_suffix_for_partial() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("/he", 3);
        let items = provider.compute(&ctx);
        let help = items
            .iter()
            .find(|i| i.label == "/help")
            .expect("expected /help completion");
        assert_eq!(help.ghost_suffix.as_deref(), Some("lp"));
    }

    #[test]
    fn command_exact_match_uses_name_without_slash() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("/help", 5);
        let items = provider.compute(&ctx);
        assert_eq!(items.first().map(|i| i.label.as_str()), Some("/help"));
    }

    #[test]
    fn no_completion_without_slash() {
        let provider = CommandCompletionProvider::new();
        let ctx = make_context("hello world", 5);
        let items = provider.compute(&ctx);
        assert!(items.is_empty(), "no slash means no command completions");
    }

    #[test]
    fn find_command_token_returns_range() {
        let result = find_command_token_range("/help me", 3);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 0..3);
    }

    #[test]
    fn find_command_token_mid_input() {
        let result = find_command_token_range("do /help now", 8);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), 3..8);
    }

    #[test]
    fn combined_completer_empty() {
        let completer = CombinedCompleter::new();
        assert!(completer.is_empty());
    }

    #[test]
    fn combined_completer_with_providers() {
        let mut completer = CombinedCompleter::new();
        completer.add_provider(Box::new(CommandCompletionProvider::new()));
        assert!(!completer.is_empty());
        assert_eq!(completer.provider_count(), 1);
    }
}
