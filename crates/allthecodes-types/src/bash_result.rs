use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Structured result of a shell-family process tool execution.
///
/// This replaces ad-hoc `json!({...})` construction with a typed struct
/// that preserves raw stdout, stderr, command, cwd, and exit code before any
/// truncation or rendering transformation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub struct ShellExecutionOutput {
    /// Original command string sent to Bash/PowerShell.
    pub command: Option<String>,
    /// Resolved working directory at execution time.
    pub cwd: Option<PathBuf>,
    /// The full stdout stream as captured from the process pipe.
    pub stdout: String,
    /// The full stderr stream as captured from the process pipe.
    pub stderr: String,
    /// Process exit code. `None` means an exit code could not be read.
    pub exit_code: Option<i32>,
    /// Whether the command was interrupted (timed out or cancelled).
    #[serde(default)]
    pub interrupted: bool,
    /// Termination reason when interrupted: "timeout" or "cancelled".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination: Option<String>,
    /// Error message for failures that prevented execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Backward-compatible alias for older in-flight code that still names the
/// shell result after the Bash tool.
pub type BashResult = ShellExecutionOutput;
