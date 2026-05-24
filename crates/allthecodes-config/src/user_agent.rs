//! User-Agent helpers shared by HTTP clients.
//!
//! Keep these helpers dependency-light so API, MCP, and tool crates can share
//! the same traffic identity without each crate hardcoding its own string.

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User-Agent used for Claude Code first-party service requests.
pub fn claude_code_user_agent() -> String {
    format!("claude-code/{VERSION}")
}

/// User-Agent used for Anthropic-style API requests.
///
/// The `claude-cli` prefix is kept for parity with upstream log filtering.
pub fn api_user_agent() -> String {
    let user_type = env_component("USER_TYPE").unwrap_or_else(|| "unknown".to_string());
    let entrypoint = env_component("ALLTHECODES_ENTRYPOINT").unwrap_or_else(|| "cli".to_string());
    let mut details = vec![user_type, entrypoint];

    if let Some(version) = env_component("CLAUDE_AGENT_SDK_VERSION") {
        details.push(format!("agent-sdk/{version}"));
    }
    if let Some(client_app) = env_component("CLAUDE_AGENT_SDK_CLIENT_APP") {
        details.push(format!("client-app/{client_app}"));
    }
    if let Some(workload) = env_component("ALLTHECODES_WORKLOAD") {
        details.push(format!("workload/{workload}"));
    }

    format!("claude-cli/{VERSION} ({})", details.join(", "))
}

/// User-Agent used by MCP HTTP transports and MCP OAuth discovery/token calls.
pub fn mcp_user_agent() -> String {
    let mut details = Vec::new();
    if let Some(entrypoint) = env_component("ALLTHECODES_ENTRYPOINT") {
        details.push(entrypoint);
    }
    if let Some(version) = env_component("CLAUDE_AGENT_SDK_VERSION") {
        details.push(format!("agent-sdk/{version}"));
    }
    if let Some(client_app) = env_component("CLAUDE_AGENT_SDK_CLIENT_APP") {
        details.push(format!("client-app/{client_app}"));
    }

    if details.is_empty() {
        claude_code_user_agent()
    } else {
        format!("{} ({})", claude_code_user_agent(), details.join(", "))
    }
}

/// User-Agent for WebFetch requests to arbitrary external sites.
pub fn web_fetch_user_agent() -> String {
    format!(
        "Claude-User ({}; +https://support.anthropic.com/)",
        claude_code_user_agent()
    )
}

fn env_component(name: &str) -> Option<String> {
    let value = std::env::var(name).ok()?;
    let value = sanitize_header_component(&value);
    (!value.is_empty()).then_some(value)
}

fn sanitize_header_component(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .collect()
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    const ENV_KEYS: &[&str] = &[
        "USER_TYPE",
        "ALLTHECODES_ENTRYPOINT",
        "CLAUDE_AGENT_SDK_VERSION",
        "CLAUDE_AGENT_SDK_CLIENT_APP",
        "ALLTHECODES_WORKLOAD",
    ];

    fn clear_env() {
        for key in ENV_KEYS {
            std::env::remove_var(key);
        }
    }

    #[test]
    #[serial]
    fn claude_code_user_agent_uses_package_version() {
        assert_eq!(
            claude_code_user_agent(),
            format!("claude-code/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    #[serial]
    fn api_user_agent_defaults_to_cli_identity() {
        clear_env();

        assert_eq!(
            api_user_agent(),
            format!("claude-cli/{} (unknown, cli)", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    #[serial]
    fn api_user_agent_includes_sdk_client_and_workload_parts() {
        clear_env();
        std::env::set_var("USER_TYPE", "pro");
        std::env::set_var("ALLTHECODES_ENTRYPOINT", "sdk");
        std::env::set_var("CLAUDE_AGENT_SDK_VERSION", "1.2.3");
        std::env::set_var("CLAUDE_AGENT_SDK_CLIENT_APP", "my-app/2");
        std::env::set_var("ALLTHECODES_WORKLOAD", "cron");

        assert_eq!(
            api_user_agent(),
            format!(
                "claude-cli/{} (pro, sdk, agent-sdk/1.2.3, client-app/my-app/2, workload/cron)",
                env!("CARGO_PKG_VERSION")
            )
        );

        clear_env();
    }

    #[test]
    #[serial]
    fn mcp_user_agent_includes_only_mcp_supported_suffixes() {
        clear_env();
        std::env::set_var("ALLTHECODES_ENTRYPOINT", "cli");
        std::env::set_var("CLAUDE_AGENT_SDK_VERSION", "1.2.3");
        std::env::set_var("CLAUDE_AGENT_SDK_CLIENT_APP", "my-app/2");

        assert_eq!(
            mcp_user_agent(),
            format!(
                "claude-code/{} (cli, agent-sdk/1.2.3, client-app/my-app/2)",
                env!("CARGO_PKG_VERSION")
            )
        );

        clear_env();
    }

    #[test]
    #[serial]
    fn web_fetch_user_agent_uses_public_fetch_identity() {
        clear_env();

        assert_eq!(
            web_fetch_user_agent(),
            format!(
                "Claude-User (claude-code/{}; +https://support.anthropic.com/)",
                env!("CARGO_PKG_VERSION")
            )
        );
    }
}
