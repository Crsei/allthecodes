//! Constants used throughout the application.
//!
//! Consolidates model identifiers, API version strings, token limits,
//! file size limits, timeout values, and other important constants
//! referenced by the TypeScript source in `src/constants/`.

// =============================================================================
// Model identifiers
// =============================================================================

/// Current frontier model IDs by tier.
pub mod models {
    /// Claude Opus 4.6 — frontier model.
    pub const OPUS: &str = "claude-opus-4-6";
    /// Claude Sonnet 4.6 — balanced model.
    pub const SONNET: &str = "claude-sonnet-4-6";
    /// Claude Haiku 4.5 — fast, lightweight model.
    pub const HAIKU: &str = "claude-haiku-4-5-20251001";

    /// Friendly display name for the frontier model.
    pub const FRONTIER_MODEL_NAME: &str = "Claude Opus 4.6";

    /// Return a human-readable tier name from a canonical model ID.
    pub fn model_tier(canonical: &str) -> &'static str {
        if canonical.contains("claude-opus-4-6") {
            "opus (4.6)"
        } else if canonical.contains("claude-opus-4-5") {
            "opus (4.5)"
        } else if canonical.contains("claude-opus-4") {
            "opus (4)"
        } else if canonical.contains("claude-sonnet-4-6") {
            "sonnet (4.6)"
        } else if canonical.contains("claude-sonnet-4-5") {
            "sonnet (4.5)"
        } else if canonical.contains("claude-sonnet-4") {
            "sonnet (4)"
        } else if canonical.contains("claude-haiku") {
            "haiku"
        } else {
            "unknown"
        }
    }
}

/// Get a marketing-friendly display name for a model ID.
///
/// Corresponds to TS: `getMarketingNameForModel(modelId)`
pub fn marketing_name_for_model(model_id: &str) -> Option<&str> {
    if model_id.contains("claude-opus-4-6") {
        Some("Claude Opus 4.6 (1M context)")
    } else if model_id.contains("claude-opus-4-5") {
        Some("Claude Opus 4.5")
    } else if model_id.contains("claude-opus-4") {
        Some("Claude Opus 4")
    } else if model_id.contains("claude-sonnet-4-6") {
        Some("Claude Sonnet 4.6")
    } else if model_id.contains("claude-sonnet-4-5") {
        Some("Claude Sonnet 4.5")
    } else if model_id.contains("claude-sonnet-4") {
        Some("Claude Sonnet 4")
    } else if model_id.contains("claude-haiku-4-5") {
        Some("Claude Haiku 4.5")
    } else {
        None
    }
}

/// Get the knowledge cutoff date string for a model.
///
/// Corresponds to TS: `getKnowledgeCutoff(modelId)`
pub fn knowledge_cutoff(model_id: &str) -> Option<&'static str> {
    if model_id.contains("claude-opus-4-6") || model_id.contains("claude-sonnet-4-6") {
        Some("May 2025")
    } else if model_id.contains("claude-sonnet-4")
        || model_id.contains("claude-opus-4")
        || model_id.contains("claude-haiku-4-5")
    {
        Some("April 2025")
    } else {
        None
    }
}

// =============================================================================
// API version strings & beta headers
// =============================================================================

pub mod api {
    /// Core beta header for Claude Code.
    /// Note: this is an Anthropic protocol header value — the constant name and
    /// value stay as-is even after the `allthecodes` rename.
    pub const CLAUDE_CODE_BETA_HEADER: &str = "claude-code-20250219";
    /// Interleaved thinking beta.
    pub const INTERLEAVED_THINKING_BETA: &str = "interleaved-thinking-2025-05-14";
    /// 1M context window beta.
    pub const CONTEXT_1M_BETA: &str = "context-1m-2025-08-07";
    /// Context management beta.
    pub const CONTEXT_MANAGEMENT_BETA: &str = "context-management-2025-06-27";
    /// Web search beta.
    pub const WEB_SEARCH_BETA: &str = "web-search-2025-03-05";
    /// Structured outputs beta.
    pub const STRUCTURED_OUTPUTS_BETA: &str = "structured-outputs-2025-12-15";
    /// Tool search beta (1P — Anthropic/Foundry).
    pub const TOOL_SEARCH_BETA_1P: &str = "advanced-tool-use-2025-11-20";
    /// Tool search beta (3P — Vertex/Bedrock).
    pub const TOOL_SEARCH_BETA_3P: &str = "tool-search-tool-2025-10-19";
    /// Effort control beta.
    pub const EFFORT_BETA: &str = "effort-2025-11-24";
    /// Token-efficient tools beta.
    pub const TOKEN_EFFICIENT_TOOLS_BETA: &str = "token-efficient-tools-2026-03-28";
    /// Prompt caching scope beta.
    pub const PROMPT_CACHING_SCOPE_BETA: &str = "prompt-caching-scope-2026-01-05";
    /// Fast mode beta.
    pub const FAST_MODE_BETA: &str = "fast-mode-2026-02-01";
    /// Redact thinking beta.
    pub const REDACT_THINKING_BETA: &str = "redact-thinking-2026-02-12";
}

// =============================================================================
// Context window & token limits
// =============================================================================

pub mod tokens {
    /// Default context window for all current Claude models (200k tokens).
    pub const MODEL_CONTEXT_WINDOW_DEFAULT: u64 = 200_000;

    /// Extended context window (1M tokens) for supported models.
    pub const CONTEXT_WINDOW_1M: u64 = 1_000_000;

    /// Default maximum output tokens per response.
    pub const MAX_OUTPUT_TOKENS_DEFAULT: u64 = 32_000;

    /// Upper-limit cap for output tokens.
    pub const MAX_OUTPUT_TOKENS_UPPER_LIMIT: u64 = 64_000;

    /// Capped default for slot-reservation optimization (p99 output ~ 5k tokens).
    pub const CAPPED_DEFAULT_MAX_TOKENS: u64 = 8_000;

    /// Escalated max tokens for retry after hitting the capped limit.
    pub const ESCALATED_MAX_TOKENS: u64 = 64_000;

    /// Maximum output tokens reserved for compact summaries.
    pub const COMPACT_MAX_OUTPUT_TOKENS: u64 = 20_000;

    /// Auto-compact trigger buffer (tokens before the context window boundary).
    pub const AUTOCOMPACT_BUFFER_TOKENS: u64 = 13_000;

    /// Warning threshold buffer (tokens remaining before showing a warning).
    pub const WARNING_THRESHOLD_BUFFER_TOKENS: u64 = 20_000;

    /// Manual compact buffer.
    pub const MANUAL_COMPACT_BUFFER_TOKENS: u64 = 3_000;

    /// Average characters per token for rough estimation.
    pub const CHARS_PER_TOKEN: f64 = 4.0;

    /// Bytes per token estimate for calculating token count from byte size.
    pub const BYTES_PER_TOKEN: u64 = 4;

    /// Maximum tool result size in tokens.
    pub const MAX_TOOL_RESULT_TOKENS: u64 = 100_000;

    /// Maximum tool result size in bytes (derived from token limit).
    pub const MAX_TOOL_RESULT_BYTES: u64 = MAX_TOOL_RESULT_TOKENS * BYTES_PER_TOKEN;
}

// =============================================================================
// Tool result limits
// =============================================================================

pub mod tool_limits {
    /// Default maximum size in characters for tool results before persisting to disk.
    pub const DEFAULT_MAX_RESULT_SIZE_CHARS: usize = 50_000;

    /// Maximum aggregate tool result chars per single user message (one turn).
    pub const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;

    /// Maximum character length for tool summary strings in compact views.
    pub const TOOL_SUMMARY_MAX_LENGTH: usize = 50;
}

// =============================================================================
// Bash / shell limits
// =============================================================================

pub mod bash {
    use std::time::Duration;

    /// Default timeout for bash command execution (2 minutes).
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(120_000);

    /// Maximum allowed timeout for bash commands (10 minutes).
    pub const MAX_TIMEOUT: Duration = Duration::from_millis(600_000);

    /// Maximum output length for bash commands (chars).
    pub const MAX_OUTPUT_DEFAULT: usize = 30_000;

    /// Upper limit for bash output (chars).
    pub const MAX_OUTPUT_UPPER_LIMIT: usize = 150_000;

    /// Maximum command length for tree-sitter parsing.
    pub const MAX_COMMAND_LENGTH: usize = 10_000;

    /// Get the default bash timeout, checking `BASH_DEFAULT_TIMEOUT_MS` env var.
    pub fn default_timeout() -> Duration {
        if let Ok(val) = std::env::var("BASH_DEFAULT_TIMEOUT_MS") {
            if let Ok(ms) = val.parse::<u64>() {
                if ms > 0 {
                    return Duration::from_millis(ms);
                }
            }
        }
        DEFAULT_TIMEOUT
    }

    /// Get the maximum bash timeout, checking `BASH_MAX_TIMEOUT_MS` env var.
    pub fn max_timeout() -> Duration {
        let default = default_timeout();
        if let Ok(val) = std::env::var("BASH_MAX_TIMEOUT_MS") {
            if let Ok(ms) = val.parse::<u64>() {
                if ms > 0 {
                    let env_dur = Duration::from_millis(ms);
                    return if env_dur > default { env_dur } else { default };
                }
            }
        }
        if MAX_TIMEOUT > default {
            MAX_TIMEOUT
        } else {
            default
        }
    }
}

// =============================================================================
// File limits & binary detection
// =============================================================================

pub mod files {
    /// Number of bytes to read for binary content detection.
    pub const BINARY_CHECK_SIZE: usize = 8192;

    /// Maximum file size for git untracked file capture (500 MB).
    pub const MAX_FILE_SIZE_BYTES: u64 = 500 * 1024 * 1024;

    /// Binary file extensions to skip for text-based operations.
    pub const BINARY_EXTENSIONS: &[&str] = &[
        // Images
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".webp", ".tiff", ".tif",
        // Videos
        ".mp4", ".mov", ".avi", ".mkv", ".webm", ".wmv", ".flv", ".m4v", ".mpeg", ".mpg",
        // Audio
        ".mp3", ".wav", ".ogg", ".flac", ".aac", ".m4a", ".wma", ".aiff", ".opus",
        // Archives
        ".zip", ".tar", ".gz", ".bz2", ".7z", ".rar", ".xz", ".z", ".tgz", ".iso",
        // Executables/binaries
        ".exe", ".dll", ".so", ".dylib", ".bin", ".o", ".a", ".obj", ".lib", ".app", ".msi", ".deb",
        ".rpm", // Documents
        ".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".odt", ".ods", ".odp",
        // Fonts
        ".ttf", ".otf", ".woff", ".woff2", ".eot", // Bytecode / VM artifacts
        ".pyc", ".pyo", ".class", ".jar", ".war", ".ear", ".node", ".wasm", ".rlib",
        // Database files
        ".sqlite", ".sqlite3", ".db", ".mdb", ".idx", // Design / 3D
        ".psd", ".ai", ".eps", ".sketch", ".fig", ".xd", ".blend", ".3ds", ".max",
        // Flash
        ".swf", ".fla", // Lock/profiling data
        ".lockb", ".dat", ".data",
    ];

    /// Check if a file path has a known binary extension.
    pub fn has_binary_extension(file_path: &str) -> bool {
        let lower = file_path.to_lowercase();
        BINARY_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
    }

    /// Check if a buffer likely contains binary content.
    ///
    /// Looks for null bytes or a high proportion of non-printable characters
    /// in the first `BINARY_CHECK_SIZE` bytes.
    pub fn is_binary_content(buffer: &[u8]) -> bool {
        let check_size = buffer.len().min(BINARY_CHECK_SIZE);
        if check_size == 0 {
            return false;
        }

        let mut non_printable = 0usize;
        for &byte in &buffer[..check_size] {
            // Null byte is a strong indicator of binary
            if byte == 0 {
                return true;
            }
            // Count non-printable, non-whitespace bytes
            if byte < 32 && byte != 9 && byte != 10 && byte != 13 {
                non_printable += 1;
            }
        }

        // If more than 10% non-printable, likely binary
        (non_printable as f64 / check_size as f64) > 0.1
    }
}

// =============================================================================
// Image & PDF limits
// =============================================================================

pub mod media {
    /// Maximum base64-encoded image size (5 MB, API enforced).
    pub const API_IMAGE_MAX_BASE64_SIZE: usize = 5 * 1024 * 1024;

    /// Target raw image size (3.75 MB — stays under base64 limit after encoding).
    pub const IMAGE_TARGET_RAW_SIZE: usize = (API_IMAGE_MAX_BASE64_SIZE * 3) / 4;

    /// Client-side maximum image dimensions.
    pub const IMAGE_MAX_WIDTH: u32 = 2000;
    pub const IMAGE_MAX_HEIGHT: u32 = 2000;

    /// Maximum raw PDF file size (20 MB).
    pub const PDF_TARGET_RAW_SIZE: usize = 20 * 1024 * 1024;

    /// Maximum pages in a PDF accepted by the API.
    pub const API_PDF_MAX_PAGES: u32 = 100;

    /// PDF size threshold for extraction to page images (3 MB).
    pub const PDF_EXTRACT_SIZE_THRESHOLD: usize = 3 * 1024 * 1024;

    /// Maximum PDF file size for the page extraction path (100 MB).
    pub const PDF_MAX_EXTRACT_SIZE: usize = 100 * 1024 * 1024;

    /// Max pages the Read tool will extract in a single call.
    pub const PDF_MAX_PAGES_PER_READ: u32 = 20;

    /// Maximum media items (images + PDFs) per API request.
    pub const API_MAX_MEDIA_PER_REQUEST: u32 = 100;
}

// =============================================================================
// Product & URL constants
// =============================================================================

pub mod product {
    /// Product URL.
    pub const PRODUCT_URL: &str = "https://claude.com/claude-code";

    /// Claude AI base URL (production).
    pub const CLAUDE_AI_BASE_URL: &str = "https://claude.ai";

    /// System prompt prefix for interactive CLI mode.
    pub const CLI_SYSPROMPT_PREFIX: &str = "You are allthecodes, a coding CLI.";

    /// System prompt prefix for Agent SDK (with allthecodes preset).
    pub const AGENT_SDK_CC_PREFIX: &str =
        "You are allthecodes, a coding CLI, running within the Agent SDK.";

    /// System prompt prefix for plain Agent SDK usage.
    pub const AGENT_SDK_PREFIX: &str = "You are an agent built for allthecodes.";

    /// Message returned when a tool call produces no content.
    pub const NO_CONTENT_MESSAGE: &str = "(no content)";
}

// =============================================================================
// XML tag constants
// =============================================================================

pub mod xml {
    pub const COMMAND_NAME_TAG: &str = "command-name";
    pub const COMMAND_MESSAGE_TAG: &str = "command-message";
    pub const COMMAND_ARGS_TAG: &str = "command-args";

    pub const BASH_INPUT_TAG: &str = "bash-input";
    pub const BASH_STDOUT_TAG: &str = "bash-stdout";
    pub const BASH_STDERR_TAG: &str = "bash-stderr";
    pub const LOCAL_COMMAND_STDOUT_TAG: &str = "local-command-stdout";
    pub const LOCAL_COMMAND_STDERR_TAG: &str = "local-command-stderr";

    pub const TASK_NOTIFICATION_TAG: &str = "task-notification";
    pub const TEAMMATE_MESSAGE_TAG: &str = "teammate-message";
}

// =============================================================================
// Tool names
// =============================================================================

pub mod tool_names {
    pub const BASH: &str = "Bash";
    pub const FILE_READ: &str = "Read";
    pub const FILE_WRITE: &str = "Write";
    pub const FILE_EDIT: &str = "Edit";
    pub const GREP: &str = "Grep";
    pub const GLOB: &str = "Glob";
    pub const AGENT: &str = "Agent";
    pub const SKILL: &str = "Skill";
    pub const WEB_SEARCH: &str = "WebSearch";
    pub const WEB_FETCH: &str = "WebFetch";
    pub const NOTEBOOK_EDIT: &str = "NotebookEdit";
    pub const TOOL_SEARCH: &str = "ToolSearch";
    pub const TODO_WRITE: &str = "TodoWrite";
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_tier() {
        assert_eq!(models::model_tier("claude-opus-4-6"), "opus (4.6)");
        assert_eq!(models::model_tier("claude-sonnet-4-6"), "sonnet (4.6)");
        assert_eq!(models::model_tier("claude-haiku-4-5-20251001"), "haiku");
        assert_eq!(models::model_tier("some-other-model"), "unknown");
    }

    #[test]
    fn test_has_binary_extension() {
        assert!(files::has_binary_extension("image.PNG"));
        assert!(files::has_binary_extension("/path/to/file.exe"));
        assert!(files::has_binary_extension("archive.tar.gz"));
        assert!(!files::has_binary_extension("code.rs"));
        assert!(!files::has_binary_extension("readme.md"));
    }

    #[test]
    fn test_is_binary_content() {
        assert!(files::is_binary_content(&[0x00, 0x01, 0x02]));
        assert!(!files::is_binary_content(b"hello world\n"));
        assert!(!files::is_binary_content(&[]));
    }

    #[test]
    fn test_bash_default_timeout() {
        // Without env var, should return the default 120s
        let timeout = bash::DEFAULT_TIMEOUT;
        assert_eq!(timeout.as_millis(), 120_000);
    }

    #[test]
    fn test_token_constants() {
        assert_eq!(tokens::MODEL_CONTEXT_WINDOW_DEFAULT, 200_000);
        assert_eq!(tokens::MAX_TOOL_RESULT_BYTES, 400_000);
    }
}
