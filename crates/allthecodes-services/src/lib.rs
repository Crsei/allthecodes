//! Background / utility services extracted from the root crate.

pub mod agent_definitions;
pub mod file_search;
pub mod langfuse;
pub mod lsp_lifecycle;
pub mod onboarding;
pub mod prompt_suggestion;
pub mod scheduler;
pub mod session_analytics;
pub mod session_memory;
#[cfg(feature = "telemetry")]
pub mod telemetry;
pub mod tool_use_summary;
