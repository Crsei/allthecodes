use std::collections::HashMap;

use allthecodes_ipc_protocol::subsystem_types::ConfigScope;
use allthecodes_mcp::McpOAuthConfig;

// ---------------------------------------------------------------------------
// Flag parsing helpers
// ---------------------------------------------------------------------------

#[derive(Default, Debug, Clone)]
pub(super) struct ParsedFlags {
    pub(super) command: Option<String>,
    pub(super) args: Vec<String>,
    pub(super) env: HashMap<String, String>,
    pub(super) url: Option<String>,
    pub(super) transport: Option<String>,
    pub(super) scope: Option<ConfigScope>,
    pub(super) oauth_auth_server_metadata_url: Option<String>,
    pub(super) oauth_client_id: Option<String>,
    pub(super) oauth_callback_port: Option<u16>,
    pub(super) oauth_scopes: Vec<String>,
    pub(super) oauth_resource: Option<String>,
    pub(super) browser: Option<bool>,
    pub(super) header: Vec<(String, String)>,
    pub(super) bearer_token_env_var: Option<String>,
    pub(super) env_http_header: Vec<(String, String)>,
    pub(super) auth: Option<String>,
    pub(super) error: Option<String>,
}

#[derive(Default, Debug, Clone)]
pub(super) struct ParsedMcpjsonDecisionArgs {
    pub(super) names: Vec<String>,
    pub(super) all_project: bool,
    pub(super) error: Option<String>,
}

#[derive(Default, Debug, Clone)]
pub(super) struct ParsedAuthCompleteArgs {
    pub(super) code: Option<String>,
    pub(super) state: Option<String>,
    pub(super) error: Option<String>,
}

impl ParsedFlags {
    fn oauth_touched(&self) -> bool {
        self.oauth_auth_server_metadata_url.is_some()
            || self.oauth_client_id.is_some()
            || self.oauth_callback_port.is_some()
            || !self.oauth_scopes.is_empty()
            || self.oauth_resource.is_some()
    }
}

pub(super) fn parse_flags(rest: &[&str]) -> ParsedFlags {
    let mut out = ParsedFlags::default();

    for raw in rest {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if let Some(stripped) = raw.strip_prefix("--command=") {
            out.command = Some(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--arg=") {
            out.args.push(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--env=") {
            if let Some(eq) = stripped.find('=') {
                let (k, v) = stripped.split_at(eq);
                out.env.insert(k.to_string(), v[1..].to_string());
            } else {
                out.error = Some(format!("malformed --env value: {}", stripped));
            }
        } else if let Some(stripped) = raw.strip_prefix("--url=") {
            out.url = Some(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--transport=") {
            out.transport = Some(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--scope=") {
            out.scope = Some(match stripped {
                "user" => ConfigScope::User,
                "project" => ConfigScope::Project,
                other => {
                    out.error = Some(format!(
                        "invalid --scope `{}` (expected user|project)",
                        other
                    ));
                    ConfigScope::User
                }
            });
        } else if let Some(stripped) = raw.strip_prefix("--oauth-auth-server-metadata-url=") {
            out.oauth_auth_server_metadata_url = Some(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--oauth-client-id=") {
            out.oauth_client_id = Some(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--oauth-callback-port=") {
            match stripped.parse::<u16>() {
                Ok(port) => out.oauth_callback_port = Some(port),
                Err(_) => {
                    out.error = Some(format!(
                        "invalid --oauth-callback-port `{}` (expected 1-65535)",
                        stripped
                    ));
                }
            }
        } else if let Some(stripped) = raw.strip_prefix("--oauth-scope=") {
            if stripped.trim().is_empty() {
                out.error = Some("--oauth-scope must not be empty".to_string());
            } else {
                out.oauth_scopes.push(stripped.to_string());
            }
        } else if raw == "--browser" {
            out.browser = Some(true);
        } else if let Some(stripped) = raw.strip_prefix("--browser=") {
            out.browser = match stripped {
                "true" | "1" | "yes" => Some(true),
                "false" | "0" | "no" => Some(false),
                other => {
                    out.error = Some(format!("invalid --browser value `{}`", other));
                    None
                }
            };
        } else if let Some(stripped) = raw.strip_prefix("--header=") {
            if let Some(eq) = stripped.find('=') {
                let (k, v) = stripped.split_at(eq);
                if is_sensitive_static_header(k) {
                    out.error = Some(format!(
                        "refusing static sensitive HTTP header `{}`; use --bearer-token-env-var or --env-http-header instead",
                        k
                    ));
                } else {
                    out.header.push((k.to_string(), v[1..].to_string()));
                }
            } else {
                out.error = Some(format!("malformed --header value: {}", stripped));
            }
        } else if let Some(stripped) = raw.strip_prefix("--bearer-token-env-var=") {
            if stripped.trim().is_empty() {
                out.error = Some("--bearer-token-env-var must not be empty".to_string());
            } else {
                out.bearer_token_env_var = Some(stripped.to_string());
            }
        } else if let Some(stripped) = raw.strip_prefix("--env-http-header=") {
            if let Some(eq) = stripped.find('=') {
                let (k, v) = stripped.split_at(eq);
                out.env_http_header
                    .push((k.to_string(), v[1..].to_string()));
            } else {
                out.error = Some(format!(
                    "malformed --env-http-header value: {} (expected Header=EnvVar)",
                    stripped
                ));
            }
        } else if let Some(stripped) = raw.strip_prefix("--oauth-resource=") {
            if stripped.trim().is_empty() {
                out.error = Some("--oauth-resource must not be empty".to_string());
            } else {
                out.oauth_resource = Some(stripped.to_string());
            }
        } else if let Some(stripped) = raw.strip_prefix("--auth=") {
            match stripped {
                "oauth" => out.auth = Some(stripped.to_string()),
                "chatgpt" => {
                    out.error =
                        Some("auth=chatgpt is reserved but not implemented yet".to_string());
                }
                other => {
                    out.error = Some(format!("invalid --auth `{}` (expected oauth)", other));
                }
            }
        } else {
            out.error = Some(format!("unknown flag `{}`", raw));
        }
    }

    out
}

fn is_sensitive_static_header(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "authorization"
        || lower == "proxy-authorization"
        || lower == "x-api-key"
        || lower == "api-key"
        || lower == "x-auth-token"
        || lower == "x-access-token"
        || lower.contains("token")
        || lower.contains("secret")
}

pub(super) fn parse_auth_complete_args(rest: &[&str]) -> ParsedAuthCompleteArgs {
    let mut out = ParsedAuthCompleteArgs::default();
    for raw in rest {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if let Some(stripped) = raw.strip_prefix("--code=") {
            out.code = Some(stripped.to_string());
        } else if let Some(stripped) = raw.strip_prefix("--state=") {
            out.state = Some(stripped.to_string());
        } else if raw.starts_with("--") {
            out.error = Some(format!("unknown flag `{}`", raw));
        } else if out.code.is_none() {
            out.code = Some(raw.to_string());
        } else {
            out.error = Some(format!("unexpected OAuth auth argument `{}`", raw));
        }
    }
    out
}

pub(super) fn oauth_from_flags(
    flags: &ParsedFlags,
    current: Option<&McpOAuthConfig>,
) -> Option<McpOAuthConfig> {
    if !flags.oauth_touched() {
        return None;
    }
    let mut oauth = current.cloned().unwrap_or_default();
    if let Some(value) = &flags.oauth_auth_server_metadata_url {
        oauth.auth_server_metadata_url = Some(value.clone());
    }
    if let Some(value) = &flags.oauth_client_id {
        oauth.client_id = Some(value.clone());
    }
    if let Some(value) = flags.oauth_callback_port {
        oauth.callback_port = Some(value);
    }
    if !flags.oauth_scopes.is_empty() {
        oauth.scopes = Some(flags.oauth_scopes.clone());
    }
    if let Some(value) = &flags.oauth_resource {
        oauth.oauth_resource = Some(value.clone());
    }
    Some(oauth)
}

pub(super) fn parse_mcpjson_decision_args(rest: &[&str]) -> ParsedMcpjsonDecisionArgs {
    let mut out = ParsedMcpjsonDecisionArgs::default();

    for raw in rest {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        if raw == "--all-project" {
            out.all_project = true;
        } else if raw.starts_with("--") {
            out.error = Some(format!("unknown flag `{}`", raw));
        } else {
            out.names.push(raw.to_string());
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_rejects_static_sensitive_header() {
        let parsed = parse_flags(&["--header=Authorization=Bearer secret"]);

        assert!(parsed.error.unwrap().contains("refusing static sensitive HTTP header"));
    }

    #[test]
    fn parse_flags_accepts_env_http_header_for_sensitive_names() {
        let parsed = parse_flags(&["--env-http-header=Authorization=MCP_TOKEN"]);

        assert!(parsed.error.is_none());
        assert_eq!(
            parsed.env_http_header,
            vec![("Authorization".to_string(), "MCP_TOKEN".to_string())]
        );
    }

    #[test]
    fn parse_flags_rejects_reserved_chatgpt_auth() {
        let parsed = parse_flags(&["--auth=chatgpt"]);

        assert_eq!(
            parsed.error.as_deref(),
            Some("auth=chatgpt is reserved but not implemented yet")
        );
    }
}
