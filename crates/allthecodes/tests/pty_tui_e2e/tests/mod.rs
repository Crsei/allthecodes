#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

pub(crate) const SCRIPTS_LOG_ROOT: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/logs/pty_tui_e2e_scripts");

mod commands_agent_team;
mod commands_aliases;
mod commands_auth;
mod commands_core_info;
mod commands_git;
mod commands_kairos;
mod commands_mcp_plugin;
mod commands_memory_skills_hooks;
mod commands_permissions;
mod commands_query;
mod commands_session;
mod commands_surface;
mod running_task_slash_commands;
mod test1_login_structure;
mod test2_full_access;
mod test3_plan_flow;
mod test4_task_execution;
mod test5_compact;
