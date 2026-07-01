//! Replaced by `operation_grouping.rs`.
//! Re-exports needed symbols for backward compatibility during transition.

pub(crate) use super::operation_grouping::{
    is_api_error_message, is_suppressed_operation_tool, is_suppressed_tool_result, tool_result_id,
    tool_use_id, uuid_from_parent_and_salt,
};
