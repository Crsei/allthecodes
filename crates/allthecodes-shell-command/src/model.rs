//! Shared DTOs for shell command parsing and analysis.
//!
//! These types form the contract between parser, permissions, sandbox, and
//! runtime layers. No runtime state or execution logic lives here.

/// Shell dialect / provider kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellDialect {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Cmd,
    Sh,
    Unknown,
}

impl ShellDialect {
    pub fn display_name(&self) -> &'static str {
        match self {
            ShellDialect::Bash => "bash",
            ShellDialect::Zsh => "zsh",
            ShellDialect::Fish => "fish",
            ShellDialect::PowerShell => "powershell",
            ShellDialect::Cmd => "cmd",
            ShellDialect::Sh => "sh",
            ShellDialect::Unknown => "unknown",
        }
    }

    /// Whether this dialect uses POSIX-compatible syntax.
    pub fn is_posix(&self) -> bool {
        matches!(
            self,
            ShellDialect::Bash | ShellDialect::Zsh | ShellDialect::Sh
        )
    }
}

/// Parse mode for security-sensitive decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseMode {
    /// Permissive parsing for display, suggestions, and non-security paths.
    /// May fall back to simple shell-words splitting on parse errors.
    Permissive,
    /// Fail-closed parsing for security decisions.
    /// Must reject on parse failure, budget overrun, or unsupported nodes.
    FailClosedSecurity,
}

// ---------------------------------------------------------------------------
// Parsed command model
// ---------------------------------------------------------------------------

/// A fully parsed shell command, split into segments with metadata.
#[derive(Debug, Clone)]
pub struct ParsedShellCommand {
    /// Original command string.
    pub original: String,
    /// Segments from compound command splitting (&&, ||, ;, |).
    pub segments: Vec<ShellSegment>,
    /// Parse diagnostics (warnings / errors).
    pub diagnostics: Vec<ParseDiagnostic>,
}

impl ParsedShellCommand {
    /// All simple commands across all segments.
    pub fn all_commands(&self) -> Vec<&SimpleCommand> {
        self.segments
            .iter()
            .filter_map(|s| s.command.as_ref())
            .collect()
    }

    /// The first simple command (for single-command shells).
    pub fn primary_command(&self) -> Option<&SimpleCommand> {
        self.segments.first().and_then(|s| s.command.as_ref())
    }

    /// Whether any segment contains a heredoc.
    pub fn has_heredoc(&self) -> bool {
        self.segments.iter().any(|s| !s.heredocs.is_empty())
    }

    /// Whether any segment has an explicit stdin redirect.
    pub fn has_stdin_redirect(&self) -> bool {
        self.segments
            .iter()
            .any(|s| s.redirections.iter().any(|r| r.operator == "<"))
    }

    /// Whether stdin redirect should be added.
    /// Returns false if heredocs exist or a stdin redirect already exists.
    pub fn needs_stdin_redirect(&self) -> bool {
        !self.has_heredoc() && !self.has_stdin_redirect()
    }
}

/// A single segment in a compound command.
#[derive(Debug, Clone)]
pub struct ShellSegment {
    /// Raw text of this segment.
    pub raw: String,
    /// Parsed simple command (if parseable).
    pub command: Option<SimpleCommand>,
    /// Redirections found in this segment.
    pub redirections: Vec<Redirection>,
    /// Heredocs found in this segment.
    pub heredocs: Vec<Heredoc>,
    /// Whether this segment is a pipeline member (|, |&).
    pub is_pipeline: bool,
    /// The separator that preceded this segment (e.g. "&&", "||", ";", "|").
    pub separator: Option<String>,
}

/// A simple command: env vars, command name, arguments.
#[derive(Debug, Clone)]
pub struct SimpleCommand {
    /// Command name (basename only).
    pub command_name: Option<String>,
    /// Command name as typed (may include path).
    pub command_raw: Option<String>,
    /// Arguments after the command (excluding env vars).
    pub args: Vec<String>,
    /// Environment variable assignments (VAR=val) before the command.
    pub env_vars: Vec<(String, String)>,
    /// Full argv: env vars skipped, command + args included.
    pub argv: Vec<String>,
}

impl SimpleCommand {
    /// Whether this command appears to be a single builtin/command with no args.
    pub fn is_bare(&self) -> bool {
        self.command_name.is_some() && self.args.is_empty()
    }
}

/// A file descriptor or file redirection.
#[derive(Debug, Clone)]
pub struct Redirection {
    /// Redirect operator (e.g. ">", "<", ">>", "2>", "2>&1").
    pub operator: String,
    /// Target file path, fd number, or /dev/null.
    pub target: String,
}

/// A heredoc specification.
#[derive(Debug, Clone)]
pub struct Heredoc {
    /// Delimiter text.
    pub delimiter: String,
    /// Whether to strip leading tabs (<<-).
    pub strip_tabs: bool,
    /// Whether delimiter is quoted ('EOF' or "EOF").
    pub quoted: bool,
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// Severity of a parse diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Warning,
    Error,
}

/// A diagnostic from shell parsing.
#[derive(Debug, Clone)]
pub struct ParseDiagnostic {
    pub message: String,
    pub severity: DiagnosticSeverity,
}

impl ParseDiagnostic {
    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            severity: DiagnosticSeverity::Error,
        }
    }

    pub fn warning(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
            severity: DiagnosticSeverity::Warning,
        }
    }
}

// ---------------------------------------------------------------------------
// Read-only command classification result
// ---------------------------------------------------------------------------

/// Result of classifying a shell command as read-only or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOnlyResult {
    /// Command is classified as read-only.
    ReadOnly,
    /// Command is not read-only, with a reason.
    NotReadOnly(String),
    /// Classification is unsupported for this command/dialect.
    Unsupported(String),
    /// The command could not be parsed.
    ParseFailed(String),
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors from parsing shell commands.
#[derive(Debug, thiserror::Error)]
pub enum ShellParseError {
    #[error("Shell parse error: {0}")]
    ParseFailed(String),
    #[error("Unterminated quotes in command")]
    UnterminatedQuotes,
    #[error("Heredoc error: {0}")]
    HeredocError(String),
    #[error("Shell-words parse error: {0}")]
    ShellWords(#[from] shell_words::ParseError),
}
