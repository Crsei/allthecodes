//! Stable task-domain vocabulary shared by task tools and command surfaces.
//!
//! These string values are persisted and exposed in tool schemas; keep them
//! byte-for-byte stable unless a migration updates all readers and writers.

pub const DEFAULT_TASK_LIST_ID: &str = "tasklist";

pub const TASK_KIND_TOOL: &str = "tool";
pub const TASK_KIND_LOCAL_BASH: &str = "local_bash";
pub const TASK_KIND_LOCAL_AGENT: &str = "local_agent";
pub const TASK_KIND_REMOTE_AGENT: &str = "remote_agent";
pub const TASK_KIND_IN_PROCESS_TEAMMATE: &str = "in_process_teammate";
pub const TASK_KIND_LOCAL_WORKFLOW: &str = "local_workflow";
pub const TASK_KIND_MONITOR_MCP: &str = "monitor_mcp";
pub const TASK_KIND_DREAM: &str = "dream";

pub const REMOTE_TASK_TYPE_REMOTE_AGENT: &str = "remote-agent";
pub const REMOTE_TASK_TYPE_ULTRAPLAN: &str = "ultraplan";
pub const REMOTE_TASK_TYPE_ULTRAREVIEW: &str = "ultrareview";
pub const REMOTE_TASK_TYPE_AUTOFIX_PR: &str = "autofix-pr";
pub const REMOTE_TASK_TYPE_BACKGROUND_PR: &str = "background-pr";

pub const TASK_CREATE_KIND_ENUM: &[&str] = &[
    TASK_KIND_TOOL,
    TASK_KIND_LOCAL_BASH,
    TASK_KIND_LOCAL_AGENT,
    TASK_KIND_REMOTE_AGENT,
    TASK_KIND_IN_PROCESS_TEAMMATE,
    TASK_KIND_LOCAL_WORKFLOW,
    TASK_KIND_MONITOR_MCP,
    TASK_KIND_DREAM,
];

pub const REMOTE_TASK_TYPE_ENUM: &[&str] = &[
    REMOTE_TASK_TYPE_REMOTE_AGENT,
    REMOTE_TASK_TYPE_ULTRAPLAN,
    REMOTE_TASK_TYPE_ULTRAREVIEW,
    REMOTE_TASK_TYPE_AUTOFIX_PR,
    REMOTE_TASK_TYPE_BACKGROUND_PR,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_domain_exports_match_persisted_schema_values() {
        assert_eq!(DEFAULT_TASK_LIST_ID, "tasklist");
        assert_eq!(TASK_KIND_REMOTE_AGENT, "remote_agent");
        assert_eq!(REMOTE_TASK_TYPE_ULTRAREVIEW, "ultrareview");
        assert!(TASK_CREATE_KIND_ENUM.contains(&TASK_KIND_IN_PROCESS_TEAMMATE));
        assert!(REMOTE_TASK_TYPE_ENUM.contains(&REMOTE_TASK_TYPE_BACKGROUND_PR));
    }
}
