//! Effective sandbox policy — merged from [`allthecodes_config::settings::SandboxSettings`]
//! plus permission rules, then cached on [`crate::types::app_state::AppState`].
//!
//! Policy assembly is deliberately decoupled from settings parsing so
//! callers (CLI `--no-network`, `/sandbox` command, session-level
//! overrides) can mutate the effective policy without re-serializing
//! settings.

use std::path::PathBuf;

use allthecodes_config::settings::SandboxSettings;

use super::availability::{detect_availability, Availability};
use super::filesystem::PathResolver;
use super::mode::SandboxMode;
use super::network::NetworkPolicy;

/// Fully-resolved sandbox policy — ready for enforcement.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    pub enabled: bool,
    pub mode: SandboxMode,
    pub fail_if_unavailable: bool,
    pub allow_unsandboxed_commands: bool,
    pub excluded_commands: Vec<String>,
    pub allowed_commands: Vec<String>,
    pub paths: PathResolver,
    pub network: NetworkPolicy,
    pub availability: Availability,
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        SandboxPolicyBuilder::new(PathBuf::from(".")).build()
    }
}

impl SandboxPolicy {
    /// Is the sandbox enabled *and* active (non-`full`)?
    pub fn is_active(&self) -> bool {
        self.enabled && self.mode.is_active()
    }

    /// Is `cmd` in the `excludedCommands` list?
    ///
    /// Matches:
    /// - `docker` → matches `docker` and `docker run ...`
    /// - `docker *` → same as `docker` (prefix on argv)
    /// - `make test` → matches `make test` and `make test -j4`
    pub fn is_excluded_command(&self, cmd: &str) -> bool {
        command_matches_any(cmd, &self.excluded_commands)
    }

    /// Is `cmd` in the `allowedCommands` pre-approval list?
    ///
    /// Exposed on the public policy surface so the permission-decision flow
    /// can auto-approve sandboxed commands in `workspace` mode.
    pub fn is_allowed_command(&self, cmd: &str) -> bool {
        allowed_command_matches_any(cmd, &self.allowed_commands)
    }
}

/// Compatibility matcher used by `excludedCommands`.
///
/// A rule matches when `cmd`:
/// - equals the rule, OR
/// - starts with `rule ` (space-separated prefix)
///
/// The trailing `*` / ` *` in a rule is stripped before matching — they
/// mean the same thing as the bare rule here.
fn command_matches_any(cmd: &str, rules: &[String]) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return false;
    }
    for rule in rules {
        let r = rule.trim().trim_end_matches('*').trim_end().to_string();
        if r.is_empty() {
            continue;
        }
        if cmd == r || cmd.starts_with(&format!("{} ", r)) {
            return true;
        }
    }
    false
}

/// Strict matcher for `allowedCommands` permission pre-approval.
///
/// Unlike `excludedCommands`, this is fail-closed: every simple command in a
/// shell chain or pipeline must parse into argv and match an allowlisted argv
/// prefix. Shell constructs that are not argv-shaped are not pre-approved.
fn allowed_command_matches_any(cmd: &str, rules: &[String]) -> bool {
    let cmd = cmd.trim();
    if cmd.is_empty() || rules.is_empty() || contains_unstructured_shell_expansion(cmd) {
        return false;
    }

    let segments = split_shell_segments(cmd);
    !segments.is_empty()
        && segments
            .iter()
            .all(|segment| simple_command_matches_any_rule(segment, rules))
}

fn simple_command_matches_any_rule(segment: &str, rules: &[String]) -> bool {
    let Ok(argv) = allthecodes_utils::bash::parse_command(segment) else {
        return false;
    };
    if argv.is_empty() || argv.iter().any(|word| is_shell_control_word(word)) {
        return false;
    }

    rules.iter().any(|rule| {
        let rule = rule.trim().trim_end_matches('*').trim_end();
        if rule.is_empty() {
            return false;
        }
        let Ok(rule_argv) = allthecodes_utils::bash::parse_command(rule) else {
            return false;
        };
        !rule_argv.is_empty()
            && argv.len() >= rule_argv.len()
            && argv
                .iter()
                .zip(rule_argv.iter())
                .all(|(actual, expected)| actual == expected)
    })
}

fn contains_unstructured_shell_expansion(cmd: &str) -> bool {
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    let chars = cmd.chars().collect::<Vec<_>>();
    let mut idx = 0;

    while idx < chars.len() {
        let ch = chars[idx];
        if escaped {
            escaped = false;
            idx += 1;
            continue;
        }
        if ch == '\\' && !in_single_quote {
            escaped = true;
            idx += 1;
            continue;
        }
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            idx += 1;
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            idx += 1;
            continue;
        }
        if !in_single_quote && !in_double_quote {
            if ch == '`' || ch == '<' || ch == '>' {
                return true;
            }
            if ch == '$' && chars.get(idx + 1) == Some(&'(') {
                return true;
            }
        }
        idx += 1;
    }

    false
}

fn is_shell_control_word(word: &str) -> bool {
    matches!(
        word,
        "&&" | "||" | "|" | ";" | "&" | ">" | ">>" | "<" | "<<" | "<<<" | "&>" | "&>>"
    ) || word.starts_with('>')
        || word.starts_with('<')
}

fn split_shell_segments(command: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' && !in_single_quote {
            current.push(ch);
            if let Some(next) = chars.next() {
                current.push(next);
            }
            continue;
        }
        if ch == '\'' && !in_double_quote {
            in_single_quote = !in_single_quote;
            current.push(ch);
            continue;
        }
        if ch == '"' && !in_single_quote {
            in_double_quote = !in_double_quote;
            current.push(ch);
            continue;
        }

        if !in_single_quote && !in_double_quote {
            if (ch == '&' || ch == '|') && chars.peek() == Some(&ch) {
                push_segment(&mut segments, &mut current);
                chars.next();
                continue;
            }
            if matches!(ch, ';' | '|') {
                push_segment(&mut segments, &mut current);
                continue;
            }
        }

        current.push(ch);
    }
    push_segment(&mut segments, &mut current);
    segments
}

fn push_segment(segments: &mut Vec<String>, current: &mut String) {
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        segments.push(trimmed.to_string());
    }
    current.clear();
}

/// Build a [`SandboxPolicy`] from raw settings + runtime overrides.
pub struct SandboxPolicyBuilder {
    workspace: PathBuf,
    settings: SandboxSettings,
    /// From CLI `--no-network` flag.
    force_no_network: bool,
    /// From `permissions.additionalDirectories` and session `addDir` grants.
    extra_workspaces: Vec<PathBuf>,
    /// Merged allow-read rules sourced from `Read(...)` permission allows.
    perm_allow_reads: Vec<String>,
    /// Merged allow-write rules sourced from `Edit(...)` permission allows.
    perm_allow_writes: Vec<String>,
    /// Merged deny-read rules sourced from `Read(...)` permission denies.
    perm_deny_reads: Vec<String>,
    /// Merged deny-write rules sourced from `Edit(...)` permission denies.
    perm_deny_writes: Vec<String>,
}

impl SandboxPolicyBuilder {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            settings: SandboxSettings::default(),
            force_no_network: false,
            extra_workspaces: Vec::new(),
            perm_allow_reads: Vec::new(),
            perm_allow_writes: Vec::new(),
            perm_deny_reads: Vec::new(),
            perm_deny_writes: Vec::new(),
        }
    }

    pub fn settings(mut self, settings: SandboxSettings) -> Self {
        self.settings = settings;
        self
    }

    pub fn no_network(mut self, on: bool) -> Self {
        self.force_no_network = on;
        self
    }

    pub fn extra_workspaces(mut self, dirs: Vec<PathBuf>) -> Self {
        self.extra_workspaces = dirs;
        self
    }

    pub fn permission_allow_reads(mut self, rules: Vec<String>) -> Self {
        self.perm_allow_reads = rules;
        self
    }

    pub fn permission_allow_writes(mut self, rules: Vec<String>) -> Self {
        self.perm_allow_writes = rules;
        self
    }

    pub fn permission_deny_reads(mut self, rules: Vec<String>) -> Self {
        self.perm_deny_reads = rules;
        self
    }

    pub fn permission_deny_writes(mut self, rules: Vec<String>) -> Self {
        self.perm_deny_writes = rules;
        self
    }

    pub fn build(self) -> SandboxPolicy {
        let Self {
            workspace,
            settings,
            force_no_network,
            extra_workspaces,
            perm_allow_reads,
            perm_allow_writes,
            perm_deny_reads,
            perm_deny_writes,
        } = self;

        let enabled = settings.enabled.unwrap_or(false);
        let mode = settings
            .mode
            .as_deref()
            .and_then(|s| s.parse::<SandboxMode>().ok())
            .unwrap_or(SandboxMode::default_enabled());
        // If enabled=false, force mode=Full so callers can just read `.mode`.
        let effective_mode = if enabled { mode } else { SandboxMode::Full };

        let mut paths = PathResolver::new(workspace);
        for extra in extra_workspaces {
            paths.add_extra_workspace(extra);
        }
        for raw in &settings.filesystem.allow_write {
            paths.add_allow_write(raw);
        }
        for raw in &settings.filesystem.deny_write {
            paths.add_deny_write(raw);
        }
        for raw in &settings.filesystem.allow_read {
            paths.add_allow_read(raw);
        }
        for raw in &settings.filesystem.deny_read {
            paths.add_deny_read(raw);
        }
        for raw in &perm_allow_writes {
            paths.add_allow_write(raw);
        }
        for raw in &perm_allow_reads {
            paths.add_allow_read(raw);
        }
        for raw in &perm_deny_writes {
            paths.add_deny_write(raw);
        }
        for raw in &perm_deny_reads {
            paths.add_deny_read(raw);
        }

        let network = NetworkPolicy {
            disabled: force_no_network || settings.network.disabled.unwrap_or(false),
            allowed_domains: settings.network.allowed_domains.clone(),
        };

        let availability = detect_availability();

        SandboxPolicy {
            enabled,
            mode: effective_mode,
            fail_if_unavailable: settings.fail_if_unavailable.unwrap_or(false),
            allow_unsandboxed_commands: settings.allow_unsandboxed_commands.unwrap_or(true),
            excluded_commands: settings.excluded_commands.clone(),
            allowed_commands: settings.allowed_commands.clone(),
            paths,
            network,
            availability,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use allthecodes_config::settings::{SandboxFilesystemSettings, SandboxNetworkSettings};

    #[test]
    fn default_is_inactive() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj")).build();
        assert!(!p.is_active());
        assert_eq!(p.mode, SandboxMode::Full);
    }

    #[test]
    fn enabled_defaults_to_workspace() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                ..Default::default()
            })
            .build();
        assert!(p.is_active());
        assert_eq!(p.mode, SandboxMode::Workspace);
    }

    #[test]
    fn explicit_mode_wins() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                mode: Some("read-only".into()),
                ..Default::default()
            })
            .build();
        assert_eq!(p.mode, SandboxMode::ReadOnly);
    }

    #[test]
    fn no_network_cli_flag_overrides() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .no_network(true)
            .build();
        assert!(p.network.disabled);
    }

    #[test]
    fn allow_write_paths_resolve() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                filesystem: SandboxFilesystemSettings {
                    allow_write: vec!["./build".into(), "/tmp/cache".into()],
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();
        let allow = p.paths.allow_write_paths();
        assert!(allow.contains(&PathBuf::from("/proj/build")));
        assert!(allow.contains(&PathBuf::from("/tmp/cache")));
    }

    #[test]
    fn excluded_command_matches_by_name() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                excluded_commands: vec!["docker".into(), "watchman *".into()],
                ..Default::default()
            })
            .build();
        assert!(p.is_excluded_command("docker run hello"));
        assert!(p.is_excluded_command("docker"));
        assert!(p.is_excluded_command("watchman status"));
        assert!(!p.is_excluded_command("doc"));
        assert!(!p.is_excluded_command("ls"));
    }

    #[test]
    fn allowed_command_distinct_from_excluded() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                allowed_commands: vec!["make test".into()],
                ..Default::default()
            })
            .build();
        assert!(p.is_allowed_command("make test"));
        assert!(p.is_allowed_command("make test -j4"));
        assert!(!p.is_allowed_command("make install"));
    }

    #[test]
    fn allowed_command_matches_argv_prefix() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                allowed_commands: vec!["cargo test".into()],
                ..Default::default()
            })
            .build();

        assert!(p.is_allowed_command("cargo test --all"));
        assert!(!p.is_allowed_command("cargo testbed"));
    }

    #[test]
    fn allowed_command_rejects_unlisted_compound_segments() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                allowed_commands: vec!["cargo test".into(), "make test".into()],
                ..Default::default()
            })
            .build();

        assert!(!p.is_allowed_command("cargo test && rm -rf /"));
        assert!(!p.is_allowed_command("cargo test; curl https://example.test/install.sh"));
        assert!(!p.is_allowed_command("make test | sh"));
    }

    #[test]
    fn allowed_command_requires_every_pipeline_segment_to_match() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                allowed_commands: vec!["printf ok".into(), "cat".into()],
                ..Default::default()
            })
            .build();

        assert!(p.is_allowed_command("printf ok | cat"));
        assert!(!p.is_allowed_command("printf ok | sh"));
    }

    #[test]
    fn allowed_command_rejects_unstructured_shell_syntax() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                allowed_commands: vec!["cargo test".into()],
                ..Default::default()
            })
            .build();

        assert!(!p.is_allowed_command("cargo test > /tmp/out"));
        assert!(!p.is_allowed_command("cargo test $(cat args)"));
        assert!(!p.is_allowed_command("cargo test `cat args`"));
    }

    #[test]
    fn network_settings_wire_through() {
        let p = SandboxPolicyBuilder::new(PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                network: SandboxNetworkSettings {
                    allowed_domains: vec!["github.com".into()],
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();
        assert!(!p.network.disabled);
        assert_eq!(p.network.allowed_domains, vec!["github.com".to_string()]);
    }
}
