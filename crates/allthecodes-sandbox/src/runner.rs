//! [`SandboxRunner`] trait + concrete back-ends.
//!
//! Callers (`BashTool`, `PowerShellTool`) use the runner to wrap a
//! `tokio::process::Command` with OS-level isolation (bubblewrap on Linux,
//! sandbox-exec on macOS) before spawn.
//!
//! On platforms that don't have a usable primitive the runner returns an
//! "unsupported" back-end that passes the command through untouched — which,
//! per `sandbox.failIfUnavailable`, either produces a log-level warning or
//! a hard [`SandboxError::PrimitiveUnavailable`].

use std::path::PathBuf;

use tokio::process::Command;

use super::availability::Mechanism;
use super::errors::SandboxError;
use super::filesystem::FsDecision;
use super::mode::SandboxMode;
use super::network::NetworkDecision;
use super::policy::SandboxPolicy;
use allthecodes_types::permissions::{ToolPermissionContext, ToolPermissionRulesBySource};

/// A command pre-assembled for sandboxed execution.
///
/// Instead of spawning directly, the runner hands back the assembled
/// [`Command`] plus metadata so callers can log the chosen mechanism,
/// inspect the resolved argv, etc.
#[derive(Debug)]
pub struct PreparedCommand {
    pub cmd: Command,
    /// Mechanism actually used (or `None` when the command runs unsandboxed).
    /// Kept on the public surface so `/sandbox`, observability, and future
    /// diagnostics can report the chosen back-end; not every call site
    /// inspects it today.
    pub mechanism: Option<Mechanism>,
    /// Human-readable summary (e.g. "bubblewrap read-only" or "unsandboxed").
    pub description: String,
}

/// Common interface implemented by each platform back-end.
pub trait SandboxRunner: Send + Sync {
    /// Wrap `inner` with this runner's isolation mechanism.
    ///
    /// `inner` is the already-built `tokio::process::Command` for the shell
    /// invocation. Back-ends may mutate it (e.g. prepending `bwrap ...
    /// --` arguments) and return a [`PreparedCommand`].
    ///
    /// Return [`SandboxError::PrimitiveUnavailable`] when the primitive is
    /// missing and `policy.fail_if_unavailable=true`.
    fn prepare(
        &self,
        inner: Command,
        policy: &SandboxPolicy,
        workdir: &std::path::Path,
    ) -> Result<PreparedCommand, SandboxError>;
}

/// Build the appropriate runner for the current platform + policy state.
///
/// Returns `None` when the sandbox is disabled entirely (`mode=full` or
/// `enabled=false`) so callers can short-circuit without pretending to
/// sandbox.
pub fn make_runner(policy: &SandboxPolicy) -> Option<Box<dyn SandboxRunner>> {
    if !policy.is_active() {
        return None;
    }
    match policy.availability.mechanism() {
        Some(Mechanism::Bubblewrap) => Some(Box::new(BubblewrapRunner)),
        Some(Mechanism::Seatbelt) => Some(Box::new(SeatbeltRunner)),
        Some(Mechanism::WindowsRestrictedToken) | None => Some(Box::new(UnsupportedRunner)),
    }
}

/// Run best-effort in-process sandbox checks that do not depend on the
/// platform runner.
///
/// This currently covers the shell-facing network policy so `--no-network`
/// and `allowedDomains` remain effective even when a command is excluded from
/// sandbox wrapping or the OS primitive is unavailable.
pub fn preflight_shell_command(policy: &SandboxPolicy, command: &str) -> Result<(), SandboxError> {
    if let NetworkDecision::Denied(err) = policy.network.check_shell_command(command) {
        return Err(err);
    }
    if policy.is_active() {
        for raw_path in extract_shell_write_targets(command) {
            let Some(path) = resolve_shell_path(policy, &raw_path) else {
                continue;
            };
            if policy.mode == SandboxMode::ReadOnly {
                return Err(SandboxError::ReadOnlyModeDeniesWrite { path });
            }
            match policy.paths.check_write(&path) {
                FsDecision::Allowed => {}
                FsDecision::Denied { reason } if reason.starts_with("denyWrite ") => {
                    return Err(SandboxError::WriteDenied { path, rule: reason });
                }
                FsDecision::Denied { reason } if reason.starts_with("outside workspace ") => {
                    return Err(SandboxError::WriteOutsideWorkspace { path });
                }
                FsDecision::Denied { reason } => {
                    return Err(SandboxError::Policy { message: reason });
                }
            }
        }
    }
    Ok(())
}

fn extract_shell_write_targets(command: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for segment in split_shell_segments(command) {
        let Ok(words) = allthecodes_utils::bash::parse_command(&segment) else {
            continue;
        };
        collect_redirection_targets(&words, &mut targets);
        collect_write_command_targets(&words, &mut targets);
    }
    targets
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

fn collect_redirection_targets(words: &[String], targets: &mut Vec<String>) {
    let mut iter = words.iter().peekable();
    while let Some(word) = iter.next() {
        if is_redirection_operator(word) {
            if let Some(target) = iter.peek() {
                push_target(target, targets);
            }
            continue;
        }
        if let Some(target) = attached_redirection_target(word) {
            push_target(target, targets);
        }
    }
}

fn is_redirection_operator(word: &str) -> bool {
    matches!(word, ">" | ">>" | "&>" | "&>>")
        || word
            .strip_suffix('>')
            .is_some_and(|prefix| !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()))
        || word
            .strip_suffix(">>")
            .is_some_and(|prefix| !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()))
}

fn attached_redirection_target(word: &str) -> Option<&str> {
    for op in ["&>>", "&>", ">>", ">"] {
        if let Some(rest) = word.strip_prefix(op) {
            return Some(rest);
        }
    }
    let first_non_digit = word
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(idx, _)| idx)?;
    if first_non_digit > 0 {
        let rest = &word[first_non_digit..];
        if let Some(target) = rest.strip_prefix(">>").or_else(|| rest.strip_prefix('>')) {
            return Some(target);
        }
    }
    None
}

fn collect_write_command_targets(words: &[String], targets: &mut Vec<String>) {
    let Some(command) = words.first().map(|w| command_name(w)) else {
        return;
    };
    let args = &words[1..];
    match command.as_str() {
        "touch" | "mkdir" | "rm" | "rmdir" | "del" | "erase" | "remove-item" | "ri"
        | "set-content" | "add-content" | "clear-content" | "new-item" | "ni" | "out-file" => {
            collect_path_args(args, targets);
        }
        "cp" | "copy" | "copy-item" | "move" | "mv" | "move-item" | "install" => {
            if let Some(destination) = powershell_named_arg(args, &["-destination", "-dest"]) {
                push_target(destination, targets);
            } else if let Some(last) = args.iter().rev().find(|arg| !is_option(arg)) {
                push_target(last, targets);
            }
        }
        "tee" | "tee-object" => collect_path_args(args, targets),
        _ => {}
    }
}

fn collect_path_args(args: &[String], targets: &mut Vec<String>) {
    let mut idx = 0;
    let mut saw_named_path = false;
    while idx < args.len() {
        let arg = &args[idx];
        if matches!(
            arg.to_ascii_lowercase().as_str(),
            "-path" | "-literalpath" | "-filepath"
        ) {
            if let Some(next) = args.get(idx + 1) {
                push_target(next, targets);
                saw_named_path = true;
            }
            idx += 2;
            continue;
        }
        idx += 1;
    }
    if !saw_named_path {
        for arg in args.iter().filter(|arg| !is_option(arg)) {
            push_target(arg, targets);
        }
    }
}

fn powershell_named_arg<'a>(args: &'a [String], names: &[&str]) -> Option<&'a str> {
    args.windows(2).find_map(|pair| {
        names
            .iter()
            .any(|name| pair[0].eq_ignore_ascii_case(name))
            .then_some(pair[1].as_str())
    })
}

fn command_name(word: &str) -> String {
    std::path::Path::new(word)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(word)
        .to_ascii_lowercase()
}

fn is_option(arg: &str) -> bool {
    arg.starts_with('-')
}

fn push_target(raw: &str, targets: &mut Vec<String>) {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed == "-"
        || trimmed.starts_with('&')
        || trimmed.starts_with('$')
        || trimmed.contains("://")
        || trimmed.eq_ignore_ascii_case("nul")
        || trimmed.eq_ignore_ascii_case("/dev/null")
    {
        return;
    }
    targets.push(trimmed.to_string());
}

fn resolve_shell_path(policy: &SandboxPolicy, raw: &str) -> Option<PathBuf> {
    let path = raw.trim_matches('"').trim_matches('\'');
    if path.is_empty() || path.contains('*') || path.contains('?') {
        return None;
    }
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() || looks_like_windows_absolute_path(path) {
        Some(candidate)
    } else {
        Some(policy.paths.workspace().join(candidate))
    }
}

fn looks_like_windows_absolute_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/')
}

// ---------------------------------------------------------------------------
// Bubblewrap runner (Linux / WSL2)
// ---------------------------------------------------------------------------

pub struct BubblewrapRunner;

impl SandboxRunner for BubblewrapRunner {
    fn prepare(
        &self,
        mut inner: Command,
        policy: &SandboxPolicy,
        workdir: &std::path::Path,
    ) -> Result<PreparedCommand, SandboxError> {
        let mechanism = Mechanism::Bubblewrap;
        let write_ok = policy.mode.allows_writes();

        // Extract everything we need from the inner command as owned data
        // so we can construct the wrapped command without borrow conflicts.
        let (program, args, envs, cwd) = extract_command_parts(&mut inner);

        // Build the `bwrap` argv.
        let mut bwrap_args: Vec<std::ffi::OsString> = vec![
            "--die-with-parent".into(),
            "--unshare-user-try".into(),
            "--unshare-ipc".into(),
            "--unshare-pid".into(),
            "--unshare-uts".into(),
            "--unshare-cgroup-try".into(),
            "--proc".into(),
            "/proc".into(),
            "--dev".into(),
            "/dev".into(),
            "--tmpfs".into(),
            "/tmp".into(),
        ];

        // Bind / read-only — broad read access, matching the spec default.
        bwrap_args.extend([
            std::ffi::OsString::from("--ro-bind"),
            std::ffi::OsString::from("/"),
            std::ffi::OsString::from("/"),
        ]);

        // Bind workspace RW (or read-only when mode is ReadOnly)
        let bind_flag: std::ffi::OsString = if write_ok {
            "--bind".into()
        } else {
            "--ro-bind".into()
        };
        bwrap_args.push(bind_flag.clone());
        bwrap_args.push(workdir.as_os_str().to_os_string());
        bwrap_args.push(workdir.as_os_str().to_os_string());

        // Extra workspaces (always RW when write_ok).
        for extra in policy.paths.extra_workspaces() {
            bwrap_args.push(bind_flag.clone());
            bwrap_args.push(extra.as_os_str().to_os_string());
            bwrap_args.push(extra.as_os_str().to_os_string());
        }

        // Additional allowWrite paths get RW bind mounts when write_ok.
        if write_ok {
            for p in policy.paths.allow_write_paths() {
                bwrap_args.push("--bind-try".into());
                bwrap_args.push(p.as_os_str().to_os_string());
                bwrap_args.push(p.as_os_str().to_os_string());
            }
        }

        // Block network access when disabled.
        if policy.network.disabled {
            bwrap_args.push("--unshare-net".into());
        }

        // Mount denyWrite paths as read-only *overlays* if they exist.
        for p in policy.paths.deny_write_paths() {
            bwrap_args.push("--ro-bind-try".into());
            bwrap_args.push(p.as_os_str().to_os_string());
            bwrap_args.push(p.as_os_str().to_os_string());
        }

        // Chdir to the workspace inside the sandbox.
        bwrap_args.push("--chdir".into());
        bwrap_args.push(workdir.as_os_str().to_os_string());

        // End of bwrap flags — the command to run follows.
        bwrap_args.push("--".into());
        bwrap_args.push(program);
        bwrap_args.extend(args);

        // Swap the command: run bwrap with our new argv, keeping env/stdio.
        let mut wrapped = Command::new("bwrap");
        wrapped.args(&bwrap_args);
        apply_envs(&mut wrapped, &envs);
        if let Some(dir) = cwd.as_deref() {
            wrapped.current_dir(dir);
        }

        let description = format!(
            "bubblewrap {} (workdir={})",
            policy.mode.as_str(),
            workdir.display()
        );
        Ok(PreparedCommand {
            cmd: wrapped,
            mechanism: Some(mechanism),
            description,
        })
    }
}

/// Extract the program, args, envs, and current_dir from a `tokio::process::Command`
/// as owned data so the caller can rebuild it.
type CommandParts = (
    std::ffi::OsString,
    Vec<std::ffi::OsString>,
    Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>,
    Option<std::path::PathBuf>,
);

fn extract_command_parts(inner: &mut Command) -> CommandParts {
    let std_inner = inner.as_std();
    let program = std_inner.get_program().to_os_string();
    let args: Vec<std::ffi::OsString> = std_inner.get_args().map(|a| a.to_os_string()).collect();
    let envs: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)> = std_inner
        .get_envs()
        .map(|(k, v)| (k.to_os_string(), v.map(|val| val.to_os_string())))
        .collect();
    let cwd = std_inner.get_current_dir().map(|d| d.to_path_buf());
    (program, args, envs, cwd)
}

fn apply_envs(cmd: &mut Command, envs: &[(std::ffi::OsString, Option<std::ffi::OsString>)]) {
    for (k, v) in envs {
        match v {
            Some(val) => {
                cmd.env(k, val);
            }
            None => {
                cmd.env_remove(k);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Seatbelt runner (macOS)
// ---------------------------------------------------------------------------

pub struct SeatbeltRunner;

impl SandboxRunner for SeatbeltRunner {
    fn prepare(
        &self,
        mut inner: Command,
        policy: &SandboxPolicy,
        workdir: &std::path::Path,
    ) -> Result<PreparedCommand, SandboxError> {
        let mechanism = Mechanism::Seatbelt;
        let profile = build_seatbelt_profile(policy, workdir);

        let (program, args, envs, cwd) = extract_command_parts(&mut inner);

        let mut wrapped = Command::new("/usr/bin/sandbox-exec");
        wrapped.arg("-p").arg(&profile).arg(&program);
        wrapped.args(&args);
        apply_envs(&mut wrapped, &envs);
        if let Some(dir) = cwd.as_deref() {
            wrapped.current_dir(dir);
        }

        let description = format!(
            "sandbox-exec {} (workdir={})",
            policy.mode.as_str(),
            workdir.display()
        );
        Ok(PreparedCommand {
            cmd: wrapped,
            mechanism: Some(mechanism),
            description,
        })
    }
}

fn build_seatbelt_profile(policy: &SandboxPolicy, workdir: &std::path::Path) -> String {
    let mut s = String::from("(version 1)\n(deny default)\n");
    // Process basics
    s.push_str("(allow process-exec)\n(allow process-fork)\n");
    s.push_str("(allow signal (target same-sandbox))\n");
    // Broad read access
    s.push_str("(allow file-read*)\n");
    // Always allow reading workdir
    s.push_str(&format!(
        "(allow file-read* (subpath \"{}\"))\n",
        workdir.display()
    ));
    if policy.mode.allows_writes() {
        // Workspace writes
        s.push_str(&format!(
            "(allow file-write* (subpath \"{}\"))\n",
            workdir.display()
        ));
        for extra in policy.paths.extra_workspaces() {
            s.push_str(&format!(
                "(allow file-write* (subpath \"{}\"))\n",
                extra.display()
            ));
        }
        for p in policy.paths.allow_write_paths() {
            s.push_str(&format!(
                "(allow file-write* (subpath \"{}\"))\n",
                p.display()
            ));
        }
        // Temp + devnull are almost always needed for tooling
        s.push_str("(allow file-write* (subpath \"/tmp\"))\n");
        s.push_str("(allow file-write* (literal \"/dev/null\"))\n");
    }
    // Deny list always takes precedence when explicitly set
    for p in policy.paths.deny_write_paths() {
        s.push_str(&format!(
            "(deny file-write* (subpath \"{}\"))\n",
            p.display()
        ));
    }
    for p in policy.paths.deny_read_paths() {
        s.push_str(&format!(
            "(deny file-read* (subpath \"{}\"))\n",
            p.display()
        ));
    }
    // Network
    if policy.network.disabled {
        s.push_str("(deny network*)\n");
    } else {
        s.push_str("(allow network*)\n");
    }
    s
}

// ---------------------------------------------------------------------------
// Unsupported runner — no OS-level enforcement on this platform.
// ---------------------------------------------------------------------------

pub struct UnsupportedRunner;

impl SandboxRunner for UnsupportedRunner {
    fn prepare(
        &self,
        inner: Command,
        policy: &SandboxPolicy,
        workdir: &std::path::Path,
    ) -> Result<PreparedCommand, SandboxError> {
        if policy.fail_if_unavailable {
            return Err(SandboxError::PrimitiveUnavailable {
                platform: current_platform(),
                detail: policy.availability.describe(),
            });
        }
        let _ = workdir;
        tracing::warn!(
            platform = current_platform(),
            "sandbox: OS-level primitive unavailable; falling back to \
             unsandboxed execution. Rust-level policy checks still apply."
        );
        Ok(PreparedCommand {
            cmd: inner,
            mechanism: None,
            description: format!(
                "unsandboxed (warning: OS primitive unavailable on {})",
                current_platform()
            ),
        })
    }
}

fn current_platform() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

/// Helper: build a [`SandboxPolicy`] from a permission context snapshot +
/// sandbox settings + session workspace.
///
/// Exposed so tools can re-derive the policy per call (permission rules
/// may change mid-session). The caller — which still holds a full
/// `AppState` inside the root crate — peels off the two fields this
/// function actually reads; that's how cc-sandbox stays free of the root
/// crate's `AppState` type, which still drags in teams / ui / keybindings
/// subsystems that haven't moved out yet.
pub fn policy_from_app_state(
    permission_context: &ToolPermissionContext,
    sandbox_settings: &allthecodes_config::settings::SandboxSettings,
    workspace: PathBuf,
    force_no_network: bool,
) -> SandboxPolicy {
    use super::policy::SandboxPolicyBuilder;

    // Extract deny rules from the permission system for filesystem merging.
    let deny_reads = collect_permission_paths(&permission_context.always_deny_rules, &["Read"]);
    let deny_writes = collect_permission_paths(
        &permission_context.always_deny_rules,
        &["Edit", "Write", "MultiEdit", "NotebookEdit"],
    );
    let mut allow_reads =
        collect_permission_paths(&permission_context.always_allow_rules, &["Read"]);
    let mut allow_writes = collect_permission_paths(
        &permission_context.always_allow_rules,
        &["Edit", "Write", "MultiEdit", "NotebookEdit"],
    );
    allow_reads.extend(collect_permission_paths(
        &permission_context.session_allow_rules,
        &["Read"],
    ));
    allow_writes.extend(collect_permission_paths(
        &permission_context.session_allow_rules,
        &["Edit", "Write", "MultiEdit", "NotebookEdit"],
    ));

    // Additional working directories from permissions are extra workspaces.
    let mut extra_workspaces: Vec<PathBuf> = Vec::new();
    for dir in permission_context.additional_working_directories.values() {
        allow_reads.push(dir.path.clone());
        if !dir.read_only {
            allow_writes.push(dir.path.clone());
            extra_workspaces.push(PathBuf::from(&dir.path));
        }
    }

    SandboxPolicyBuilder::new(workspace)
        .settings(sandbox_settings.clone())
        .no_network(force_no_network)
        .extra_workspaces(extra_workspaces)
        .permission_allow_reads(allow_reads)
        .permission_allow_writes(allow_writes)
        .permission_deny_reads(deny_reads)
        .permission_deny_writes(deny_writes)
        .build()
}

fn collect_permission_paths(
    rules_by_source: &ToolPermissionRulesBySource,
    tool_names: &[&str],
) -> Vec<String> {
    let mut out = Vec::new();
    for rules in rules_by_source.values() {
        for rule in rules {
            let Some(path) = permission_rule_to_path(rule, tool_names) else {
                continue;
            };
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

fn permission_rule_to_path(rule: &str, tool_names: &[&str]) -> Option<String> {
    let open = rule.find('(')?;
    if !rule.ends_with(')') {
        return None;
    }
    let tool_name = rule[..open].trim();
    if !tool_names.contains(&tool_name) {
        return None;
    }
    let pattern = rule[open + 1..rule.len() - 1].trim();
    sanitize_permission_path_pattern(pattern)
}

fn sanitize_permission_path_pattern(pattern: &str) -> Option<String> {
    let mut value = pattern
        .trim()
        .strip_prefix("prefix:")
        .unwrap_or(pattern)
        .trim();
    if value.is_empty() || value == "*" {
        return None;
    }

    let trimmed_stars = value.trim_end_matches('*');
    if trimmed_stars.contains('*') {
        return None;
    }
    value = trimmed_stars;

    if value.ends_with('/') || value.ends_with('\\') {
        value = &value[..value.len() - 1];
    }
    if value.is_empty() {
        return Some("/".into());
    }
    Some(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inactive_policy_returns_no_runner() {
        let policy = SandboxPolicy::default();
        assert!(make_runner(&policy).is_none());
    }

    #[test]
    fn unsupported_runner_warns_then_passes_through() {
        let policy = SandboxPolicy::default();
        let runner = UnsupportedRunner;
        let inner = Command::new("echo");
        let prepared = runner
            .prepare(inner, &policy, std::path::Path::new("/tmp"))
            .expect("no fail_if_unavailable → pass-through");
        assert!(prepared.mechanism.is_none());
        assert!(prepared.description.contains("unsandboxed"));
    }

    #[test]
    fn unsupported_runner_hard_fails_when_configured() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::SandboxSettings;
        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/tmp"))
            .settings(SandboxSettings {
                enabled: Some(true),
                fail_if_unavailable: Some(true),
                ..Default::default()
            })
            .build();
        let runner = UnsupportedRunner;
        let inner = Command::new("echo");
        let err = runner
            .prepare(inner, &policy, std::path::Path::new("/tmp"))
            .expect_err("fail_if_unavailable → hard fail");
        assert!(matches!(err, SandboxError::PrimitiveUnavailable { .. }));
    }

    #[test]
    fn seatbelt_profile_includes_workdir() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::SandboxSettings;
        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                ..Default::default()
            })
            .build();
        let profile = build_seatbelt_profile(&policy, std::path::Path::new("/proj"));
        assert!(profile.contains("(version 1)"));
        assert!(profile.contains("/proj"));
    }

    #[test]
    fn seatbelt_profile_denies_network_when_disabled() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::{SandboxNetworkSettings, SandboxSettings};
        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                network: SandboxNetworkSettings {
                    disabled: Some(true),
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();
        let profile = build_seatbelt_profile(&policy, std::path::Path::new("/proj"));
        assert!(profile.contains("(deny network*)"));
    }

    fn default_permission_ctx() -> ToolPermissionContext {
        ToolPermissionContext {
            mode: allthecodes_types::permissions::PermissionMode::Default,
            additional_working_directories: std::collections::HashMap::new(),
            always_allow_rules: std::collections::HashMap::new(),
            always_deny_rules: std::collections::HashMap::new(),
            always_ask_rules: std::collections::HashMap::new(),
            session_allow_rules: std::collections::HashMap::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: false,
            is_auto_mode_available: None,
            pre_plan_mode: None,
        }
    }

    #[test]
    fn policy_from_app_state_picks_up_deny_rules() {
        use std::collections::HashMap;

        let mut ctx = default_permission_ctx();
        let mut rules = HashMap::new();
        rules.insert(
            "Edit".to_string(),
            vec!["Edit(/etc/passwd)".to_string(), "Read(~/.ssh)".to_string()],
        );
        ctx.always_deny_rules = rules;

        let settings = allthecodes_config::settings::SandboxSettings::default();
        let policy =
            policy_from_app_state(&ctx, &settings, std::path::PathBuf::from("/proj"), false);
        // Deny-write should have /etc/passwd
        let writes = policy.paths.deny_write_paths();
        assert!(writes.iter().any(|p| p.ends_with("passwd")));
    }

    #[test]
    fn preflight_shell_command_checks_network_policy() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::{SandboxNetworkSettings, SandboxSettings};

        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                network: SandboxNetworkSettings {
                    allowed_domains: vec!["example.com".into()],
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();

        let err = preflight_shell_command(&policy, "curl https://evil.net")
            .expect_err("disallowed host should be blocked");
        assert!(matches!(err, SandboxError::DomainNotAllowed { .. }));
    }

    #[test]
    fn preflight_shell_command_blocks_read_only_writes() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::SandboxSettings;

        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                mode: Some("read-only".into()),
                ..Default::default()
            })
            .build();

        let err = preflight_shell_command(&policy, "echo hi > out.txt")
            .expect_err("read-only mode should block explicit writes");
        assert!(matches!(err, SandboxError::ReadOnlyModeDeniesWrite { .. }));
    }

    #[test]
    fn preflight_shell_command_blocks_outside_workspace_writes() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::SandboxSettings;

        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                ..Default::default()
            })
            .build();

        let err = preflight_shell_command(&policy, "echo hi > /tmp/out.txt")
            .expect_err("workspace sandbox should block outside writes");
        assert!(matches!(err, SandboxError::WriteOutsideWorkspace { .. }));
    }

    #[test]
    fn preflight_shell_command_allows_workspace_writes() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::SandboxSettings;

        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                ..Default::default()
            })
            .build();

        preflight_shell_command(&policy, "mkdir build && touch build/out.txt")
            .expect("workspace writes should pass preflight");
    }

    #[test]
    fn preflight_shell_command_respects_allow_and_deny_write() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::{SandboxFilesystemSettings, SandboxSettings};

        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                filesystem: SandboxFilesystemSettings {
                    allow_write: vec!["/tmp/build".into()],
                    deny_write: vec!["/proj/secrets".into()],
                    ..Default::default()
                },
                ..Default::default()
            })
            .build();

        preflight_shell_command(&policy, "echo hi > /tmp/build/out.txt")
            .expect("allowWrite should permit outside write target");
        let err = preflight_shell_command(&policy, "echo hi > /proj/secrets/out.txt")
            .expect_err("denyWrite should override workspace writes");
        assert!(matches!(err, SandboxError::WriteDenied { .. }));
    }

    #[test]
    fn preflight_shell_command_checks_powershell_write_cmdlets() {
        use super::super::policy::SandboxPolicyBuilder;
        use allthecodes_config::settings::SandboxSettings;

        let policy = SandboxPolicyBuilder::new(std::path::PathBuf::from("/proj"))
            .settings(SandboxSettings {
                enabled: Some(true),
                ..Default::default()
            })
            .build();

        let err = preflight_shell_command(&policy, "Set-Content -Path /tmp/out.txt -Value hi")
            .expect_err("PowerShell write cmdlets should be checked");
        assert!(matches!(err, SandboxError::WriteOutsideWorkspace { .. }));
    }

    #[test]
    fn policy_from_app_state_picks_up_allow_rules() {
        use std::collections::HashMap;

        let mut ctx = default_permission_ctx();
        let mut rules = HashMap::new();
        rules.insert(
            "project".to_string(),
            vec![
                "Read(/opt/tools/*)".to_string(),
                "Edit(/tmp/work/*)".to_string(),
            ],
        );
        ctx.always_allow_rules = rules;

        let settings = allthecodes_config::settings::SandboxSettings::default();
        let policy =
            policy_from_app_state(&ctx, &settings, std::path::PathBuf::from("/proj"), false);
        assert!(policy
            .paths
            .allow_read_paths()
            .iter()
            .any(|p| p == &std::path::PathBuf::from("/opt/tools")));
        assert!(policy
            .paths
            .allow_write_paths()
            .iter()
            .any(|p| p == &std::path::PathBuf::from("/tmp/work")));
    }
}
