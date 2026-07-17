//! Conversation-history compaction pipeline — extracted as a workspace
//! crate in Phase 4 (issue #73).
//!
//! Depends only on `cc-types` (message/state types) and `cc-utils` (token
//! counting), so the extraction is a clean DAG addition with no reverse
//! deps into the root crate.

pub mod auto_compact;
pub mod compaction;
pub mod context_analysis;
pub mod context_collapse;
pub mod gates;
pub mod messages;
pub mod microcompact;
pub mod partial_compact;
pub mod pipeline;
pub mod session_memory_compact;
pub mod snip;
pub mod tool_result_budget;
pub(crate) mod utf8_preview;
