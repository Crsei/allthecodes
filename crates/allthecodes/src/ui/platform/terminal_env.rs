//! TUI-facing re-export of the canonical terminal environment contract.
//!
//! The command crate owns parsing so `/terminal-setup` and the runtime cannot
//! drift apart. Keeping this module preserves the existing
//! `crate::ui::terminal_env` path used by the TUI without maintaining a
//! second parser.

pub use allthecodes_commands::terminal_env::TerminalEnvConfig;
