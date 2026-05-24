//! Trait used by the input-processing layer to parse slash commands without
//! importing the main crate's `commands::` module.
//!
//! The pure parsing layer only needs two operations: parse an input string and
//! find out the canonical command name for a parsed index. The async lifecycle
//! layer still executes handlers through the concrete command registry.
//!
//! See issue #74 (`[workspace-split] Phase 5`, sub-task 5c).

/// Minimal view of a parsed slash command.
///
/// Mirrors the `(usize, String)` tuple historically returned by the main
/// crate's `parse_command_input`: the command-registry index and the raw
/// argument string (everything after the command token).
#[derive(Debug, Clone)]
pub struct ParsedCommand {
    /// Zero-based index of the command in the registry's `get_all_commands()`
    /// list. Opaque to the parser; the lifecycle layer resolves it against the
    /// concrete command registry when executing the handler.
    pub index: usize,
    /// Arguments: the trimmed text after the command token (may be empty).
    pub args: String,
}

/// Trait for parsing and looking up slash commands.
///
/// Object-safe: call sites store this as `Arc<dyn CommandDispatcher>`.
pub trait CommandDispatcher: Send + Sync {
    /// Parse a raw user input string.
    ///
    /// Returns `Some(ParsedCommand)` iff the input starts with `/` and the
    /// token after the slash resolves to a registered command name or alias.
    /// Otherwise returns `None` (including for non-slash input).
    fn parse_command_input(&self, input: &str) -> Option<ParsedCommand>;

    /// Canonical name of the command at the given registry index.
    ///
    /// Returns `None` if the index is out of range.
    fn command_name(&self, index: usize) -> Option<String>;
}

// ---------------------------------------------------------------------------
// NoopCommandDispatcher — default that never matches any input
// ---------------------------------------------------------------------------

/// A `CommandDispatcher` that never recognises any slash commands.
///
/// Used as the default dispatcher for engines constructed without an explicit
/// one (e.g. in unit tests). Real call sites override with the concrete
/// `DefaultCommandDispatcher` from the main crate.
pub struct NoopCommandDispatcher;

impl NoopCommandDispatcher {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopCommandDispatcher {
    fn default() -> Self {
        Self
    }
}

impl CommandDispatcher for NoopCommandDispatcher {
    fn parse_command_input(&self, _input: &str) -> Option<ParsedCommand> {
        None
    }

    fn command_name(&self, _index: usize) -> Option<String> {
        None
    }
}
