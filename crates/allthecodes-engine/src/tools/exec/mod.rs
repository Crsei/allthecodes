//! Execution tools sub-domain.
//!
//! Tools in this module spawn subprocesses, evaluate code in an embedded
//! runtime, or otherwise control the passage of time for the agent loop. They
//! share interrupt semantics, timeout handling, and dangerous-command
//! preflighting so those concerns stay co-located.
//!
//! See `src/tools/ARCHITECTURE.md` for placement rules.

use std::sync::Arc;

use allthecodes_engine::types::tool::Tools;

pub mod bash;
pub mod powershell;
mod powershell_parser;
pub(crate) mod process_control;
pub mod repl;

/// Returns every tool owned by the execution sub-domain.
///
/// The registry aggregates each sub-domain's `tools()` instead of hard-coding
/// the full list, so adding a new exec tool only requires touching this file.
pub fn tools() -> Tools {
    vec![
        Arc::new(bash::BashTool::new()),
        Arc::new(powershell::PowerShellTool),
        Arc::new(repl::ReplTool),
    ]
}
