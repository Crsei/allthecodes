//! CLI argument definitions (Phase A).
//!
//! Extracted from `main.rs` so the entry point is a thin orchestrator:
//! `Cli::parse()` owns argument shape, everything else owns behaviour.

use clap::Parser;
use std::path::PathBuf;

/// allthecodes CLI
#[derive(Parser, Debug)]
#[command(
    name = "claude",
    version,
    about = "allthecodes CLI",
    disable_version_flag = true
)]
pub struct Cli {
    /// Print the version and exit (fast path)
    #[arg(short = 'V', long)]
    pub version: bool,

    /// Print mode: output model response and exit (non-interactive)
    #[arg(short = 'p', long = "print")]
    pub print: bool,

    /// Resume the most recent session
    #[arg(long)]
    pub resume: bool,

    /// Continue a specific session by ID
    #[arg(long = "continue")]
    pub continue_session: Option<String>,

    /// Maximum number of turns for agentic loops
    #[arg(long)]
    pub max_turns: Option<usize>,

    /// Working directory override
    #[arg(short = 'C', long = "cwd")]
    pub cwd: Option<String>,

    /// Model override
    #[arg(short = 'm', long)]
    pub model: Option<String>,

    /// Custom system prompt (replaces default)
    #[arg(long = "system-prompt")]
    pub system_prompt: Option<String>,

    /// Append to the system prompt
    #[arg(long = "append-system-prompt")]
    pub append_system_prompt: Option<String>,

    /// Permission mode: default, auto, bypass
    #[arg(long = "permission-mode")]
    pub permission_mode: Option<String>,

    /// Verbose output
    #[arg(short, long)]
    pub verbose: bool,

    /// Maximum budget in USD
    #[arg(long = "max-budget")]
    pub max_budget: Option<f64>,

    /// Output format (text, json, stream-json)
    #[arg(long = "output-format")]
    pub output_format: Option<String>,

    /// Dump system prompt and exit (fast path, internal)
    #[arg(long = "dump-system-prompt", hide = true)]
    pub dump_system_prompt: bool,

    /// Init only: initialize and exit (fast path)
    #[arg(long = "init-only", hide = true)]
    pub init_only: bool,

    /// Headless mode: run without TUI, communicate via JSON on stdin/stdout
    #[arg(long, hide = true)]
    pub headless: bool,

    /// Agent Client Protocol mode: JSON-RPC 2.0 over stdio (ACP v2).
    #[arg(long = "acp", hide = true, conflicts_with_all = ["headless", "print", "web", "daemon"])]
    pub acp: bool,

    /// INTERNAL: export accepted Rust TUI insta snapshots into one review folder.
    #[arg(long = "export-ui-snapshots", value_name = "DIR", hide = true)]
    pub export_ui_snapshots: Option<PathBuf>,

    /// Run as a background daemon with HTTP server (KAIROS mode).
    #[arg(long, hide = true)]
    pub daemon: bool,

    /// INTERNAL: run as a daemon worker child process.
    #[arg(long = "daemon-worker", hide = true)]
    pub daemon_worker: Option<String>,

    /// INTERNAL: daemon worker identifier.
    #[arg(long = "worker-id", hide = true)]
    pub worker_id: Option<String>,

    /// Daemon HTTP port (default: 19836).
    #[arg(long, default_value = "19836")]
    pub port: u16,

    /// Enable native Computer Use tools (screenshot, click, type, key, scroll).
    /// Registers built-in desktop control tools without needing an external MCP server.
    #[arg(long = "computer-use")]
    pub computer_use: bool,

    /// Enable the first-party Chrome integration ("Claude in Chrome"). Scans
    /// for the Anthropic Chrome extension and installs the native messaging
    /// host manifest. Overrides the `CLAUDE_CODE_ENABLE_CFC` env var and the
    /// saved `claudeInChromeDefaultEnabled` config.
    #[arg(long = "chrome", conflicts_with = "no_chrome")]
    pub chrome: bool,

    /// Explicitly disable the first-party Chrome integration for this session.
    /// Overrides env and saved config.
    #[arg(long = "no-chrome")]
    pub no_chrome: bool,

    /// INTERNAL: run as a Chrome native-messaging host. Launched by Chrome
    /// via the manifest installed by the Chrome subsystem setup (see
    /// `src/browser/setup.rs`). Reads 4-byte-framed JSON from stdin, opens
    /// a local socket, bridges the two. Not intended for manual invocation.
    #[arg(long = "chrome-native-host", hide = true)]
    pub chrome_native_host: bool,

    /// INTERNAL: run as the Claude-in-Chrome stdio MCP bridge. Spawned as a
    /// subprocess of the allthecodes MCP manager when --chrome is active.
    /// Connects to the native-host socket and exposes the first-party
    /// browser tool surface via MCP.
    #[arg(long = "claude-in-chrome-mcp", hide = true)]
    pub claude_in_chrome_mcp: bool,

    /// Disable all network access for the current session. Forwarded to
    /// [`allthecodes_sandbox::SandboxPolicy`] so shell subprocesses and WebFetch
    /// are both blocked.
    #[arg(long = "no-network")]
    pub no_network: bool,

    /// Launch web UI mode (HTTP server with chat interface).
    #[arg(long)]
    pub web: bool,

    /// Port for the web UI server (default: 17322).
    #[arg(long = "web-port", default_value_t = 17322)]
    pub web_port: u16,

    /// Unified server listen URL.
    ///
    /// Overrides `--web`, `--daemon`, `--web-port`, and `--port`.
    ///
    /// Examples:
    ///   web://127.0.0.1:17322           — Web UI only
    ///   daemon://127.0.0.1:19836        — Daemon API only
    ///   all://web=127.0.0.1:17322,daemon=127.0.0.1:19836  — Both
    ///   off                             — No HTTP server (TUI mode)
    #[arg(long, hide = true)]
    pub listen: Option<String>,

    /// Do not auto-open browser when starting web UI.
    #[arg(long = "no-open")]
    pub no_open: bool,

    /// Inline prompt (positional argument or via stdin in print mode)
    pub prompt: Vec<String>,
}
