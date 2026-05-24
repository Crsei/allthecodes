//! Shared shell command parsing contract for Bash/PowerShell tools.
//!
//! This crate provides:
//! - Stable DTOs (`model`) for parsed commands, segments, redirections, heredocs
//! - A `fallback` parser using traditional shell-quote splitting (no tree-sitter)
//! - A tree-sitter Bash AST parser (`bash_ast`) with fail-closed security semantics
//! - Analysis utilities (`tree_sitter_analysis`) for quote context, compound
//!   structure, and dangerous pattern detection
//! - (future) `ShellProvider` abstraction
//!
//! # Ownership
//!
//! Per `docs/reference/CRATE_DEPENDENCY_TARGETS.md`, this crate is the natural
//! owner for shell parsing, display, risk summary, and escalation adapters.
//! It must not depend on `cc-engine`, `cc-ui`, or `allthecodes`.
//!
//! # Parse modes
//!
//! - `Permissive` — best-effort parse for display and non-security paths.
//! - `FailClosedSecurity` — strict parse for security decisions; rejects
//!   on unterminated quotes, malformed tokens, or parse failures.

pub mod bash_ast;
pub mod fallback;
pub mod heredoc;
pub mod model;
pub mod pipe;
pub mod provider;
pub mod tree_sitter_analysis;

// Re-export common types at the crate root for convenience.
pub use model::*;
