use std::fmt;

use reqwest::StatusCode;

#[derive(Debug, Clone)]
pub(super) struct McpAuthNeededError {
    pub(super) server_name: String,
    pub(super) status: StatusCode,
}

impl fmt::Display for McpAuthNeededError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "MCP server '{}' requires authentication (HTTP {}); run `/mcp auth start {}` and then `/mcp auth complete {} --code=<code>`",
            self.server_name,
            self.status.as_u16(),
            self.server_name,
            self.server_name
        )
    }
}

impl std::error::Error for McpAuthNeededError {}

pub fn is_auth_needed_error(error: &anyhow::Error) -> bool {
    error.downcast_ref::<McpAuthNeededError>().is_some()
}
