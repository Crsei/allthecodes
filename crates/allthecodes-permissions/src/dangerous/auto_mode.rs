use allthecodes_types::permissions::{
    PermissionMode, StrippedPermissionRule, ToolPermissionContext, ToolPermissionRulesBySource,
};
use allthecodes_types::tool_metadata::{ToolMetadata, ToolRisk};

const CROSS_PLATFORM_CODE_EXEC_AUTO_ALLOW_PATTERNS: &[&str] = &[
    "python", "python3", "python2", "node", "deno", "tsx", "ruby", "perl", "php", "lua", "npm",
    "yarn", "pnpm", "bun", "npx", "bunx", "npm run", "yarn run", "pnpm run", "bun run", "bash",
    "sh", "ssh",
];

const DANGEROUS_BASH_AUTO_ALLOW_PATTERNS: &[&str] =
    &["zsh", "fish", "eval", "exec", "env", "xargs", "sudo"];

const DANGEROUS_POWERSHELL_AUTO_ALLOW_PATTERNS: &[&str] = &[
    "pwsh",
    "powershell",
    "cmd",
    "wsl",
    "iex",
    "invoke-expression",
    "icm",
    "invoke-command",
    "start-process",
    "saps",
    "start",
    "start-job",
    "sajb",
    "start-threadjob",
    "register-objectevent",
    "register-engineevent",
    "register-wmievent",
    "register-scheduledjob",
    "new-pssession",
    "nsn",
    "enter-pssession",
    "etsn",
    "add-type",
    "new-object",
    "runas",
];

/// Result of removing allow rules that would bypass Auto mode classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoModePermissionStrip {
    pub sanitized_allow_rules: ToolPermissionRulesBySource,
    pub stripped_dangerous_rules: Vec<StrippedPermissionRule>,
}

/// Summary of runtime allow-rule changes made for Auto mode safety.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoModeRuntimeTransition {
    pub stripped_always_allow_count: usize,
    pub stripped_session_allow_count: usize,
    pub restored_always_allow_count: usize,
    pub restored_session_allow_count: usize,
    pub auto_mode_blocked_by_policy: bool,
}

/// Remove allow rules that are too broad or too dangerous for Auto mode.
///
/// This mirrors Bun's `stripDangerousPermissionsForAutoMode()` at the
/// permission-rule layer. It does not mutate settings; callers can persist
/// `stripped_dangerous_rules` in runtime state and restore them when leaving
/// Auto mode with [`restore_dangerous_permissions_after_auto_mode`].
pub fn strip_dangerous_permissions_for_auto_mode(
    allow_rules: &ToolPermissionRulesBySource,
) -> AutoModePermissionStrip {
    let mut sanitized_allow_rules = ToolPermissionRulesBySource::new();
    let mut stripped_dangerous_rules = Vec::new();

    for (source, rules) in allow_rules {
        let mut kept = Vec::new();
        for rule in rules {
            if let Some(reason) = dangerous_auto_mode_allow_reason(rule) {
                stripped_dangerous_rules.push(StrippedPermissionRule {
                    source: source.clone(),
                    rule: rule.clone(),
                    reason: reason.to_string(),
                });
            } else {
                kept.push(rule.clone());
            }
        }

        if !kept.is_empty() {
            sanitized_allow_rules.insert(source.clone(), kept);
        }
    }

    AutoModePermissionStrip {
        sanitized_allow_rules,
        stripped_dangerous_rules,
    }
}

/// Restore allow rules previously removed by
/// [`strip_dangerous_permissions_for_auto_mode`].
pub fn restore_dangerous_permissions_after_auto_mode(
    mut sanitized_allow_rules: ToolPermissionRulesBySource,
    stripped_dangerous_rules: &[StrippedPermissionRule],
) -> ToolPermissionRulesBySource {
    for stripped in stripped_dangerous_rules {
        let rules = sanitized_allow_rules
            .entry(stripped.source.clone())
            .or_default();
        if !rules.iter().any(|rule| rule == &stripped.rule) {
            rules.push(stripped.rule.clone());
        }
    }
    sanitized_allow_rules
}

/// Set permission mode while keeping Auto mode's broad allow-rule stripping
/// in sync with runtime state.
pub fn set_permission_mode_with_auto_mode_safety(
    ctx: &mut ToolPermissionContext,
    requested: PermissionMode,
) -> AutoModeRuntimeTransition {
    let mut transition = AutoModeRuntimeTransition::default();
    let requested_auto_blocked = requested == PermissionMode::Auto && !ctx.allows_auto_mode();
    let effective = if requested_auto_blocked {
        PermissionMode::Default
    } else {
        requested
    };

    if requested_auto_blocked {
        transition.auto_mode_blocked_by_policy = true;
    }

    if effective != PermissionMode::Auto {
        merge_transition(&mut transition, restore_auto_mode_stripped_permissions(ctx));
    }

    ctx.mode = effective;

    if ctx.mode == PermissionMode::Auto {
        merge_transition(
            &mut transition,
            strip_dangerous_permissions_for_active_auto_mode(ctx),
        );
    }

    transition
}

/// Strip any broad allow rules currently present while Auto mode is active.
///
/// This is safe to call repeatedly; only newly present dangerous rules are
/// moved into the stripped-rule side buffers.
pub fn strip_dangerous_permissions_for_active_auto_mode(
    ctx: &mut ToolPermissionContext,
) -> AutoModeRuntimeTransition {
    if ctx.mode != PermissionMode::Auto {
        return AutoModeRuntimeTransition::default();
    }

    let always = strip_dangerous_permissions_for_auto_mode(&ctx.always_allow_rules);
    let session = strip_dangerous_permissions_for_auto_mode(&ctx.session_allow_rules);
    let stripped_always_allow_count = always.stripped_dangerous_rules.len();
    let stripped_session_allow_count = session.stripped_dangerous_rules.len();

    ctx.always_allow_rules = always.sanitized_allow_rules;
    ctx.session_allow_rules = session.sanitized_allow_rules;
    ctx.auto_mode_stripped_always_allow_rules
        .extend(always.stripped_dangerous_rules);
    ctx.auto_mode_stripped_session_allow_rules
        .extend(session.stripped_dangerous_rules);

    AutoModeRuntimeTransition {
        stripped_always_allow_count,
        stripped_session_allow_count,
        ..Default::default()
    }
}

/// Restore allow rules previously stripped for Auto mode.
pub fn restore_auto_mode_stripped_permissions(
    ctx: &mut ToolPermissionContext,
) -> AutoModeRuntimeTransition {
    let stripped_always = std::mem::take(&mut ctx.auto_mode_stripped_always_allow_rules);
    let stripped_session = std::mem::take(&mut ctx.auto_mode_stripped_session_allow_rules);
    let restored_always_allow_count = stripped_always.len();
    let restored_session_allow_count = stripped_session.len();

    if !stripped_always.is_empty() {
        let current = std::mem::take(&mut ctx.always_allow_rules);
        ctx.always_allow_rules =
            restore_dangerous_permissions_after_auto_mode(current, &stripped_always);
    }
    if !stripped_session.is_empty() {
        let current = std::mem::take(&mut ctx.session_allow_rules);
        ctx.session_allow_rules =
            restore_dangerous_permissions_after_auto_mode(current, &stripped_session);
    }

    AutoModeRuntimeTransition {
        restored_always_allow_count,
        restored_session_allow_count,
        ..Default::default()
    }
}

fn merge_transition(target: &mut AutoModeRuntimeTransition, source: AutoModeRuntimeTransition) {
    target.stripped_always_allow_count += source.stripped_always_allow_count;
    target.stripped_session_allow_count += source.stripped_session_allow_count;
    target.restored_always_allow_count += source.restored_always_allow_count;
    target.restored_session_allow_count += source.restored_session_allow_count;
    target.auto_mode_blocked_by_policy |= source.auto_mode_blocked_by_policy;
}

fn dangerous_auto_mode_allow_reason(rule: &str) -> Option<&'static str> {
    let trimmed = rule.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (tool, specifier) = split_permission_rule(trimmed);
    let metadata = ToolMetadata::from_tool_name(tool);
    if !tool_requires_auto_mode_classifier_review(metadata) {
        return None;
    }

    if metadata.capabilities.spawn_agents {
        return Some("Agent allow rules bypass Auto mode classifier review");
    }

    if metadata.capabilities.run_processes {
        return match metadata.name {
            "Bash" => dangerous_shell_allow_reason(specifier, false),
            "PowerShell" => dangerous_shell_allow_reason(specifier, true),
            _ => Some("Process allow rules bypass Auto mode classifier review"),
        };
    }

    None
}

fn tool_requires_auto_mode_classifier_review(metadata: ToolMetadata) -> bool {
    metadata.risk > ToolRisk::Low
}

fn split_permission_rule(rule: &str) -> (&str, Option<&str>) {
    let Some(open) = rule.find('(') else {
        return (rule, None);
    };
    if !rule.ends_with(')') {
        return (rule, None);
    }
    let tool = rule[..open].trim();
    let specifier = rule[open + 1..rule.len() - 1].trim();
    (tool, Some(specifier))
}

fn dangerous_shell_allow_reason(specifier: Option<&str>, powershell: bool) -> Option<&'static str> {
    let Some(specifier) = specifier else {
        return Some("Blanket shell allow rules bypass Auto mode classifier review");
    };
    let normalized = normalize_auto_allow_specifier(specifier);
    if normalized.is_empty() || normalized == "*" {
        return Some("Blanket shell allow rules bypass Auto mode classifier review");
    }

    if CROSS_PLATFORM_CODE_EXEC_AUTO_ALLOW_PATTERNS
        .iter()
        .chain(DANGEROUS_BASH_AUTO_ALLOW_PATTERNS.iter())
        .any(|pattern| auto_allow_content_matches_pattern(&normalized, pattern, powershell))
    {
        return Some("Shell code execution or elevation rules bypass Auto mode classifier review");
    }

    if powershell
        && DANGEROUS_POWERSHELL_AUTO_ALLOW_PATTERNS
            .iter()
            .any(|pattern| auto_allow_content_matches_pattern(&normalized, pattern, true))
    {
        return Some(
            "PowerShell code execution or elevation rules bypass Auto mode classifier review",
        );
    }

    let first_token = normalized
        .split(|c: char| c.is_whitespace() || matches!(c, ':' | '*' | '/' | '\\'))
        .find(|part| !part.is_empty())
        .unwrap_or("");
    if matches!(
        first_token,
        "dash" | "cmd" | "cmd.exe" | "source" | "." | "su" | "doas" | "runas"
    ) {
        return Some("Shell code execution or elevation rules bypass Auto mode classifier review");
    }

    None
}

fn auto_allow_content_matches_pattern(content: &str, pattern: &str, include_exe: bool) -> bool {
    if auto_allow_content_matches_exact_pattern(content, pattern) {
        return true;
    }

    if include_exe {
        let exe_pattern = windows_exe_auto_allow_pattern(pattern);
        if auto_allow_content_matches_exact_pattern(content, &exe_pattern) {
            return true;
        }
    }

    false
}

fn auto_allow_content_matches_exact_pattern(content: &str, pattern: &str) -> bool {
    content == pattern
        || content == format!("{pattern}:*")
        || content == format!("{pattern}*")
        || content == format!("{pattern} *")
        || (content.starts_with(&format!("{pattern} -")) && content.ends_with('*'))
}

fn windows_exe_auto_allow_pattern(pattern: &str) -> String {
    if let Some((head, tail)) = pattern.split_once(' ') {
        format!("{head}.exe {tail}")
    } else {
        format!("{pattern}.exe")
    }
}

fn normalize_auto_allow_specifier(specifier: &str) -> String {
    let lower = specifier.trim().to_ascii_lowercase();
    lower
        .strip_prefix("prefix:")
        .unwrap_or(&lower)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_dangerous_permissions_for_auto_mode() {
        let mut rules = ToolPermissionRulesBySource::new();
        rules.insert(
            "user".into(),
            vec![
                "Bash(cargo test*)".into(),
                "Bash(prefix:git)".into(),
                "Bash(prefix:python)".into(),
                "Bash(prefix:npm)".into(),
                "Bash(npm run:*)".into(),
                "Bash(ssh *)".into(),
                "Bash(sudo:*)".into(),
                "PowerShell(Invoke-Expression:*)".into(),
                "PowerShell(python.exe:*)".into(),
                "Agent(*)".into(),
                "Read".into(),
            ],
        );
        rules.insert(
            "project".into(),
            vec!["Bash".into(), "PowerShell(*)".into()],
        );

        let result = strip_dangerous_permissions_for_auto_mode(&rules);

        assert_eq!(
            result.sanitized_allow_rules.get("user").unwrap(),
            &vec![
                "Bash(cargo test*)".to_string(),
                "Bash(prefix:git)".to_string(),
                "Read".to_string(),
            ]
        );
        assert!(!result.sanitized_allow_rules.contains_key("project"));

        for expected in [
            "Bash(prefix:python)",
            "Bash(prefix:npm)",
            "Bash(npm run:*)",
            "Bash(ssh *)",
            "Bash(sudo:*)",
            "PowerShell(Invoke-Expression:*)",
            "PowerShell(python.exe:*)",
            "Agent(*)",
            "Bash",
            "PowerShell(*)",
        ] {
            assert!(
                result
                    .stripped_dangerous_rules
                    .iter()
                    .any(|stripped| stripped.rule == expected),
                "expected {expected} to be stripped"
            );
        }
        assert!(result
            .stripped_dangerous_rules
            .iter()
            .all(|stripped| !stripped.reason.is_empty()));
    }

    #[test]
    fn test_strip_dangerous_permissions_keeps_narrow_shell_rules() {
        let mut rules = ToolPermissionRulesBySource::new();
        rules.insert(
            "settings".into(),
            vec![
                "Bash(prefix:git)".into(),
                "Bash(cargo clippy*)".into(),
                "PowerShell(Get-ChildItem*)".into(),
                "PowerShell(prefix:Start-Process)".into(),
                "PowerShell(npm.exe run:*)".into(),
                "PowerShell(Add-Type*)".into(),
                "Bash(prefix:node)".into(),
            ],
        );

        let result = strip_dangerous_permissions_for_auto_mode(&rules);

        assert_eq!(
            result.sanitized_allow_rules.get("settings").unwrap(),
            &vec![
                "Bash(prefix:git)".to_string(),
                "Bash(cargo clippy*)".to_string(),
                "PowerShell(Get-ChildItem*)".to_string(),
            ]
        );
        assert_eq!(result.stripped_dangerous_rules.len(), 4);
        assert!(result
            .stripped_dangerous_rules
            .iter()
            .any(|stripped| stripped.rule == "PowerShell(prefix:Start-Process)"));
        assert!(result
            .stripped_dangerous_rules
            .iter()
            .any(|stripped| stripped.rule == "PowerShell(npm.exe run:*)"));
        assert!(result
            .stripped_dangerous_rules
            .iter()
            .any(|stripped| stripped.rule == "PowerShell(Add-Type*)"));
        assert!(result
            .stripped_dangerous_rules
            .iter()
            .any(|stripped| stripped.rule == "Bash(prefix:node)"));
    }

    #[test]
    fn test_restore_dangerous_permissions_after_auto_mode() {
        let stripped = vec![
            StrippedPermissionRule {
                source: "user".into(),
                rule: "Bash(prefix:python)".into(),
                reason: "dangerous".into(),
            },
            StrippedPermissionRule {
                source: "project".into(),
                rule: "Agent(*)".into(),
                reason: "dangerous".into(),
            },
            StrippedPermissionRule {
                source: "user".into(),
                rule: "Bash(prefix:python)".into(),
                reason: "duplicate".into(),
            },
        ];
        let mut sanitized = ToolPermissionRulesBySource::new();
        sanitized.insert(
            "user".into(),
            vec!["Read".into(), "Bash(prefix:python)".into()],
        );

        let restored = restore_dangerous_permissions_after_auto_mode(sanitized, &stripped);

        assert_eq!(
            restored.get("user").unwrap(),
            &vec!["Read".to_string(), "Bash(prefix:python)".to_string()]
        );
        assert_eq!(
            restored.get("project").unwrap(),
            &vec!["Agent(*)".to_string()]
        );
    }

    fn test_permission_context(mode: PermissionMode) -> ToolPermissionContext {
        ToolPermissionContext {
            mode,
            additional_working_directories: std::collections::HashMap::new(),
            always_allow_rules: ToolPermissionRulesBySource::new(),
            always_deny_rules: ToolPermissionRulesBySource::new(),
            always_ask_rules: ToolPermissionRulesBySource::new(),
            session_allow_rules: ToolPermissionRulesBySource::new(),
            auto_mode_stripped_always_allow_rules: Vec::new(),
            auto_mode_stripped_session_allow_rules: Vec::new(),
            is_bypass_permissions_mode_available: true,
            is_auto_mode_available: Some(true),
            pre_plan_mode: None,
        }
    }

    #[test]
    fn test_auto_mode_runtime_transition_strips_and_restores() {
        let mut ctx = test_permission_context(PermissionMode::Default);
        ctx.always_allow_rules.insert(
            "user".into(),
            vec!["Bash".into(), "Bash(cargo test*)".into()],
        );
        ctx.session_allow_rules.insert(
            "session".into(),
            vec!["PowerShell(*)".into(), "Read".into()],
        );

        let stripped = set_permission_mode_with_auto_mode_safety(&mut ctx, PermissionMode::Auto);

        assert_eq!(ctx.mode, PermissionMode::Auto);
        assert_eq!(stripped.stripped_always_allow_count, 1);
        assert_eq!(stripped.stripped_session_allow_count, 1);
        assert_eq!(
            ctx.always_allow_rules.get("user").unwrap(),
            &vec!["Bash(cargo test*)".to_string()]
        );
        assert_eq!(
            ctx.session_allow_rules.get("session").unwrap(),
            &vec!["Read".to_string()]
        );
        assert_eq!(ctx.auto_mode_stripped_always_allow_rules.len(), 1);
        assert_eq!(ctx.auto_mode_stripped_session_allow_rules.len(), 1);

        let repeated = set_permission_mode_with_auto_mode_safety(&mut ctx, PermissionMode::Auto);
        assert_eq!(repeated.stripped_always_allow_count, 0);
        assert_eq!(repeated.stripped_session_allow_count, 0);
        assert_eq!(ctx.auto_mode_stripped_always_allow_rules.len(), 1);

        let restored = set_permission_mode_with_auto_mode_safety(&mut ctx, PermissionMode::Default);

        assert_eq!(ctx.mode, PermissionMode::Default);
        assert_eq!(restored.restored_always_allow_count, 1);
        assert_eq!(restored.restored_session_allow_count, 1);
        assert!(ctx.auto_mode_stripped_always_allow_rules.is_empty());
        assert!(ctx.auto_mode_stripped_session_allow_rules.is_empty());
        assert_eq!(
            ctx.always_allow_rules.get("user").unwrap(),
            &vec!["Bash(cargo test*)".to_string(), "Bash".to_string()]
        );
        assert_eq!(
            ctx.session_allow_rules.get("session").unwrap(),
            &vec!["Read".to_string(), "PowerShell(*)".to_string()]
        );
    }

    #[test]
    fn test_auto_mode_runtime_transition_respects_availability_policy() {
        let mut ctx = test_permission_context(PermissionMode::Default);
        ctx.is_auto_mode_available = Some(false);
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash".into()]);

        let transition = set_permission_mode_with_auto_mode_safety(&mut ctx, PermissionMode::Auto);

        assert!(transition.auto_mode_blocked_by_policy);
        assert_eq!(ctx.mode, PermissionMode::Default);
        assert_eq!(
            ctx.always_allow_rules.get("user").unwrap(),
            &vec!["Bash".to_string()]
        );
        assert!(ctx.auto_mode_stripped_always_allow_rules.is_empty());
    }

    #[test]
    fn test_auto_mode_policy_disable_restores_active_auto_mode_rules() {
        let mut ctx = test_permission_context(PermissionMode::Default);
        ctx.always_allow_rules
            .insert("user".into(), vec!["Bash".into()]);
        set_permission_mode_with_auto_mode_safety(&mut ctx, PermissionMode::Auto);
        assert_eq!(ctx.mode, PermissionMode::Auto);
        assert_eq!(ctx.auto_mode_stripped_always_allow_rules.len(), 1);

        ctx.is_auto_mode_available = Some(false);
        let transition = set_permission_mode_with_auto_mode_safety(&mut ctx, PermissionMode::Auto);

        assert!(transition.auto_mode_blocked_by_policy);
        assert_eq!(ctx.mode, PermissionMode::Default);
        assert!(ctx.auto_mode_stripped_always_allow_rules.is_empty());
        assert_eq!(
            ctx.always_allow_rules.get("user").unwrap(),
            &vec!["Bash".to_string()]
        );
    }

    #[test]
    fn test_active_auto_mode_strips_new_session_rules() {
        let mut ctx = test_permission_context(PermissionMode::Auto);
        ctx.session_allow_rules
            .insert("session".into(), vec!["Bash(cargo test*)".into()]);
        let initial = strip_dangerous_permissions_for_active_auto_mode(&mut ctx);
        assert_eq!(initial.stripped_session_allow_count, 0);

        ctx.session_allow_rules
            .entry("session".into())
            .or_default()
            .push("Agent(*)".into());
        let stripped = strip_dangerous_permissions_for_active_auto_mode(&mut ctx);

        assert_eq!(stripped.stripped_session_allow_count, 1);
        assert_eq!(
            ctx.session_allow_rules.get("session").unwrap(),
            &vec!["Bash(cargo test*)".to_string()]
        );
        assert_eq!(ctx.auto_mode_stripped_session_allow_rules.len(), 1);
    }

    #[test]
    fn test_auto_mode_rule_stripping_covers_spawn_agent_metadata() {
        let mut rules = ToolPermissionRulesBySource::new();
        rules.insert(
            "user".into(),
            vec![
                "Agent(*)".into(),
                "TeamSpawn(*)".into(),
                "FollowupTask(*)".into(),
                "Read".into(),
            ],
        );

        let result = strip_dangerous_permissions_for_auto_mode(&rules);

        assert_eq!(
            result.sanitized_allow_rules.get("user").unwrap(),
            &vec!["Read".to_string()]
        );
        for expected in ["Agent(*)", "TeamSpawn(*)", "FollowupTask(*)"] {
            assert!(
                result
                    .stripped_dangerous_rules
                    .iter()
                    .any(|stripped| stripped.rule == expected),
                "expected {expected} to be stripped"
            );
        }
    }

    #[test]
    fn test_auto_mode_rule_stripping_uses_tool_policy_metadata() {
        assert!(tool_requires_auto_mode_classifier_review(
            ToolMetadata::from_tool_name("Agent")
        ));
        assert!(tool_requires_auto_mode_classifier_review(
            ToolMetadata::from_tool_name("Bash")
        ));
        assert!(tool_requires_auto_mode_classifier_review(
            ToolMetadata::from_tool_name("PowerShell")
        ));
        assert!(!tool_requires_auto_mode_classifier_review(
            ToolMetadata::from_tool_name("Read")
        ));
    }
}
