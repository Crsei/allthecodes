//! Background / utility services extracted from the root crate.

pub mod agent_definitions;
pub mod agent_runtime_history;
pub mod chat_modes;
pub mod cost_ledger;
pub mod dream;
pub mod file_search;
pub mod langfuse;
pub mod lsp_lifecycle;
pub mod onboarding;
pub mod proactive;
pub mod prompt_suggestion;
pub mod scheduler;
pub mod scheduler_tools;
pub mod search_tips;
pub mod session_analytics;
pub mod session_memory;
pub mod skill_search_prefetch;
#[cfg(feature = "telemetry")]
pub mod telemetry;
pub mod tool_use_summary;
