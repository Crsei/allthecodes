//! Network policy — domain allowlist + `--no-network` gating.
//!
//! Shared by web fetch callers and the shell sandboxes.

use super::errors::SandboxError;
use allthecodes_utils::bash::{parse_command, split_compound_command};
use regex::Regex;
use std::sync::LazyLock;

/// Outcome of a network check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkDecision {
    Allowed,
    Denied(SandboxError),
}

/// Compiled network policy.
#[derive(Debug, Clone, Default)]
pub struct NetworkPolicy {
    /// If `true`, all outbound requests are blocked.
    pub disabled: bool,
    /// If non-empty, only hosts matching one of these patterns are allowed.
    ///
    /// Patterns:
    /// - Bare hostname → exact match
    /// - `*.example.com` → matches `x.example.com` and `example.com` itself
    /// - `example.com` → matches `example.com` AND its subdomains
    ///   (matches the Claude Code spec behavior)
    pub allowed_domains: Vec<String>,
}

impl NetworkPolicy {
    pub fn check_host(&self, host: &str) -> NetworkDecision {
        if self.disabled {
            return NetworkDecision::Denied(SandboxError::NetworkDisabled);
        }
        if self.allowed_domains.is_empty() {
            return NetworkDecision::Allowed;
        }
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        for pattern in &self.allowed_domains {
            if matches_domain(&host, pattern) {
                return NetworkDecision::Allowed;
            }
        }
        NetworkDecision::Denied(SandboxError::DomainNotAllowed { host })
    }

    /// Convenience check for full URLs. Extracts the host and delegates.
    pub fn check_url(&self, url: &str) -> NetworkDecision {
        if self.disabled {
            return NetworkDecision::Denied(SandboxError::NetworkDisabled);
        }
        match ::url::Url::parse(url) {
            Ok(u) => match u.host_str() {
                Some(h) => self.check_host(h),
                None => NetworkDecision::Denied(SandboxError::Policy {
                    message: format!("URL has no host: {}", url),
                }),
            },
            Err(_) => NetworkDecision::Denied(SandboxError::Policy {
                message: format!("invalid URL: {}", url),
            }),
        }
    }

    /// Best-effort network preflight for shell commands.
    ///
    /// This closes the gap between `WebFetch` policy checks and shell
    /// subprocesses by inspecting common network-oriented command shapes
    /// (`curl`, `wget`, `git clone`, `ssh`, etc.) before spawn. When
    /// `allowedDomains` is non-empty and the command appears networked but no
    /// host can be derived, we fail closed rather than silently allowing an
    /// unsandboxed network escape.
    pub fn check_shell_command(&self, command: &str) -> NetworkDecision {
        if !self.disabled && self.allowed_domains.is_empty() {
            return NetworkDecision::Allowed;
        }

        for subcommand in split_compound_command(command) {
            let analysis = analyze_shell_network(&subcommand);
            if !analysis.requires_network {
                continue;
            }

            if self.disabled {
                return NetworkDecision::Denied(SandboxError::NetworkDisabled);
            }

            if analysis.hosts.is_empty() {
                return NetworkDecision::Denied(SandboxError::Policy {
                    message: format!(
                        "command may access the network but no target host could be derived: {}",
                        subcommand.trim()
                    ),
                });
            }

            for host in analysis.hosts {
                if let NetworkDecision::Denied(err) = self.check_host(&host) {
                    return NetworkDecision::Denied(err);
                }
            }
        }

        NetworkDecision::Allowed
    }
}

fn matches_domain(host: &str, pattern: &str) -> bool {
    let pat = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
    if pat.is_empty() {
        return false;
    }
    if let Some(rest) = pat.strip_prefix("*.") {
        // `*.example.com` → matches `foo.example.com` and `example.com`
        return host == rest || host.ends_with(&format!(".{}", rest));
    }
    // Bare `example.com` → matches itself + subdomains
    host == pat || host.ends_with(&format!(".{}", pat))
}

#[derive(Debug, Default)]
struct ShellNetworkAnalysis {
    requires_network: bool,
    hosts: Vec<String>,
}

fn analyze_shell_network(command: &str) -> ShellNetworkAnalysis {
    let mut analysis = ShellNetworkAnalysis {
        requires_network: false,
        hosts: extract_hosts_from_text(command),
    };

    if !analysis.hosts.is_empty() {
        analysis.requires_network = true;
    }

    let Ok(words) = parse_command(command) else {
        dedupe_hosts(&mut analysis.hosts);
        return analysis;
    };

    let Some((cmd, args)) = split_command_and_args(&words) else {
        dedupe_hosts(&mut analysis.hosts);
        return analysis;
    };

    let cmd = cmd.to_ascii_lowercase();
    match cmd.as_str() {
        "curl" | "wget" | "http" | "httpie" | "ftp" => {
            analysis.requires_network = true;
            analysis
                .hosts
                .extend(args.iter().filter_map(|arg| extract_host(arg)));
        }
        "ssh" | "scp" | "sftp" => {
            analysis.requires_network = true;
            analysis
                .hosts
                .extend(args.iter().filter_map(|arg| extract_remote_host(arg)));
        }
        "rsync" => {
            analysis.requires_network = true;
            analysis
                .hosts
                .extend(args.iter().filter_map(|arg| extract_remote_host(arg)));
        }
        "ping" | "telnet" | "nc" | "ncat" | "netcat" => {
            analysis.requires_network = true;
            analysis.hosts.extend(
                args.iter()
                    .filter(|arg| !arg.starts_with('-'))
                    .filter_map(|arg| extract_host(arg)),
            );
        }
        "git" => {
            let git = analyze_git_command(args);
            analysis.requires_network |= git.requires_network;
            analysis.hosts.extend(git.hosts);
        }
        "npm" | "pnpm" | "yarn" | "bun" => {
            if first_non_flag(args).is_some_and(|sub| {
                matches!(sub, "install" | "add" | "update" | "up" | "create" | "dlx")
            }) {
                analysis.requires_network = true;
                analysis
                    .hosts
                    .extend(args.iter().filter_map(|arg| extract_host(arg)));
            }
        }
        "pip" | "pip3" => {
            if first_non_flag(args)
                .is_some_and(|sub| matches!(sub, "install" | "download" | "wheel" | "index"))
            {
                analysis.requires_network = true;
                analysis
                    .hosts
                    .extend(args.iter().filter_map(|arg| extract_host(arg)));
            }
        }
        "cargo" => {
            if first_non_flag(args)
                .is_some_and(|sub| matches!(sub, "install" | "search" | "publish" | "login"))
            {
                analysis.requires_network = true;
                analysis
                    .hosts
                    .extend(args.iter().filter_map(|arg| extract_host(arg)));
            }
        }
        _ => {}
    }

    dedupe_hosts(&mut analysis.hosts);
    analysis
}

fn split_command_and_args(words: &[String]) -> Option<(&str, &[String])> {
    for (idx, word) in words.iter().enumerate() {
        if word.starts_with('-') {
            return None;
        }
        if let Some((name, _)) = word.split_once('=') {
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue;
            }
        }
        let command = std::path::Path::new(word)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(word.as_str());
        return Some((command, &words[idx + 1..]));
    }
    None
}

fn analyze_git_command(args: &[String]) -> ShellNetworkAnalysis {
    let mut analysis = ShellNetworkAnalysis::default();
    let Some(subcommand) = first_non_flag(args) else {
        return analysis;
    };

    match subcommand {
        "clone" | "fetch" | "pull" | "push" | "ls-remote" => {
            analysis.requires_network = true;
            analysis
                .hosts
                .extend(args.iter().filter_map(|arg| extract_host(arg)));
        }
        "remote" => {
            let positional: Vec<&str> = args
                .iter()
                .filter(|arg| !arg.starts_with('-'))
                .map(|arg| arg.as_str())
                .collect();
            if positional
                .get(1)
                .is_some_and(|action| matches!(*action, "add" | "set-url"))
            {
                analysis.requires_network = true;
                analysis
                    .hosts
                    .extend(positional.iter().filter_map(|arg| extract_host(arg)));
            }
        }
        "submodule" => {
            if args.iter().any(|arg| arg == "update") {
                analysis.requires_network = true;
                analysis
                    .hosts
                    .extend(args.iter().filter_map(|arg| extract_host(arg)));
            }
        }
        _ => {}
    }

    analysis
}

fn first_non_flag(args: &[String]) -> Option<&str> {
    args.iter()
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str())
}

fn extract_host(token: &str) -> Option<String> {
    if let Ok(url) = url::Url::parse(token) {
        return url.host_str().map(normalize_host);
    }
    extract_remote_host(token).or_else(|| bare_host(token))
}

fn extract_remote_host(token: &str) -> Option<String> {
    if token.starts_with('-') || token.starts_with('/') || token.starts_with("./") {
        return None;
    }
    let candidate = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '(' | ')' | ',' | ';'));
    if let Some((left, _)) = candidate.split_once(':') {
        if candidate.contains("://") {
            return None;
        }
        if left.len() == 1 && left.chars().all(|ch| ch.is_ascii_alphabetic()) {
            return None;
        }
        let host = left.rsplit_once('@').map(|(_, host)| host).unwrap_or(left);
        return bare_host(host);
    }
    None
}

fn bare_host(token: &str) -> Option<String> {
    let host = token
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '[' | ']' | '(' | ')' | ',' | ';'))
        .trim_end_matches('.');
    if host.is_empty() || host.contains('/') {
        return None;
    }
    if is_ipv4(host) || host.eq_ignore_ascii_case("localhost") {
        return Some(host.to_ascii_lowercase());
    }
    if host.contains('.')
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.'))
    {
        return Some(host.to_ascii_lowercase());
    }
    None
}

fn normalize_host(host: &str) -> String {
    host.trim_matches(|ch| matches!(ch, '[' | ']'))
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn is_ipv4(value: &str) -> bool {
    value
        .split('.')
        .all(|segment| !segment.is_empty() && segment.parse::<u8>().is_ok())
        && value.split('.').count() == 4
}

static URL_IN_TEXT_RE: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:https?|ssh|ftp|git)://[^\s'"<>()]+"#));

fn extract_hosts_from_text(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(regex) = URL_IN_TEXT_RE.as_ref() else {
        return out;
    };
    for matched in regex.find_iter(command) {
        if let Ok(url) = url::Url::parse(matched.as_str()) {
            if let Some(host) = url.host_str() {
                out.push(normalize_host(host));
            }
        }
    }
    out
}

fn dedupe_hosts(hosts: &mut Vec<String>) {
    let mut deduped = Vec::new();
    for host in hosts.drain(..) {
        if !deduped.contains(&host) {
            deduped.push(host);
        }
    }
    *hosts = deduped;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_denies_everything() {
        let p = NetworkPolicy {
            disabled: true,
            ..Default::default()
        };
        assert!(matches!(
            p.check_host("example.com"),
            NetworkDecision::Denied(SandboxError::NetworkDisabled)
        ));
    }

    #[test]
    fn empty_list_allows_everything() {
        let p = NetworkPolicy::default();
        assert_eq!(p.check_host("any.example.com"), NetworkDecision::Allowed);
    }

    #[test]
    fn bare_domain_matches_self_and_subs() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["example.com".into()],
        };
        assert_eq!(p.check_host("example.com"), NetworkDecision::Allowed);
        assert_eq!(p.check_host("api.example.com"), NetworkDecision::Allowed);
        assert!(matches!(
            p.check_host("evil.net"),
            NetworkDecision::Denied(_)
        ));
    }

    #[test]
    fn wildcard_matches_subdomains() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["*.example.com".into()],
        };
        assert_eq!(p.check_host("x.example.com"), NetworkDecision::Allowed);
        assert_eq!(p.check_host("example.com"), NetworkDecision::Allowed);
        assert!(matches!(
            p.check_host("other.net"),
            NetworkDecision::Denied(_)
        ));
    }

    #[test]
    fn host_matching_is_case_insensitive() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["EXAMPLE.com".into()],
        };
        assert_eq!(p.check_host("Example.Com"), NetworkDecision::Allowed);
    }

    #[test]
    fn url_check_uses_host() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["example.com".into()],
        };
        assert_eq!(
            p.check_url("https://api.example.com/x"),
            NetworkDecision::Allowed
        );
        assert!(matches!(
            p.check_url("https://evil.net/"),
            NetworkDecision::Denied(_)
        ));
    }

    #[test]
    fn url_check_rejects_invalid_url() {
        let p = NetworkPolicy::default();
        assert!(matches!(
            p.check_url("not a url"),
            NetworkDecision::Denied(_)
        ));
    }

    #[test]
    fn shell_check_blocks_curl_when_network_disabled() {
        let p = NetworkPolicy {
            disabled: true,
            ..Default::default()
        };
        assert!(matches!(
            p.check_shell_command("curl https://example.com"),
            NetworkDecision::Denied(SandboxError::NetworkDisabled)
        ));
    }

    #[test]
    fn shell_check_allows_matching_domain() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["example.com".into()],
        };
        assert_eq!(
            p.check_shell_command("curl https://api.example.com/v1"),
            NetworkDecision::Allowed
        );
    }

    #[test]
    fn shell_check_denies_disallowed_domain() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["example.com".into()],
        };
        assert!(matches!(
            p.check_shell_command("git clone https://evil.net/repo.git"),
            NetworkDecision::Denied(SandboxError::DomainNotAllowed { .. })
        ));
    }

    #[test]
    fn shell_check_fails_closed_for_hostless_network_command_under_allowlist() {
        let p = NetworkPolicy {
            disabled: false,
            allowed_domains: vec!["example.com".into()],
        };
        assert!(matches!(
            p.check_shell_command("npm install left-pad"),
            NetworkDecision::Denied(SandboxError::Policy { .. })
        ));
    }
}
