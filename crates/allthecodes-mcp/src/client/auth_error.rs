use std::fmt;

use reqwest::StatusCode;

/// Information extracted from a WWW-Authenticate header for an auth failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WwwAuthenticateChallenge {
    /// Full raw WWW-Authenticate header value.
    pub(super) header: String,
    /// When the Bearer error is `insufficient_scope`, the optional required scope.
    pub(super) required_scope: Option<String>,
    /// True when this challenge is a Bearer `insufficient_scope` error.
    pub(super) insufficient_scope: bool,
}

#[derive(Debug, Clone)]
pub(super) struct McpAuthNeededError {
    pub(super) server_name: String,
    pub(super) status: StatusCode,
    /// Parsed WWW-Authenticate challenge, if present in the response.
    pub(super) challenge: Option<WwwAuthenticateChallenge>,
}

impl McpAuthNeededError {
    pub(super) fn new(
        server_name: String,
        status: StatusCode,
        challenge: Option<WwwAuthenticateChallenge>,
    ) -> Self {
        Self {
            server_name,
            status,
            challenge,
        }
    }
}

impl fmt::Display for McpAuthNeededError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(challenge) = &self.challenge {
            if challenge.insufficient_scope {
                if let Some(scope) = &challenge.required_scope {
                    return write!(
                        f,
                        "MCP server '{}' returned HTTP {} with insufficient scope; required scope: '{}'",
                        self.server_name, self.status.as_u16(), scope,
                    );
                }
                return write!(
                    f,
                    "MCP server '{}' returned HTTP {} with insufficient scope",
                    self.server_name,
                    self.status.as_u16(),
                );
            }
            // Show the challenge header (redacted for length)
            let preview = if challenge.header.len() > 80 {
                format!("{}...", &challenge.header[..77])
            } else {
                challenge.header.clone()
            };
            return write!(
                f,
                "MCP server '{}' requires authentication (HTTP {}): {}",
                self.server_name,
                self.status.as_u16(),
                preview,
            );
        }
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
